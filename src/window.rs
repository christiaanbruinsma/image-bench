use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use adw::prelude::*;
use gtk::{gio, glib, Align, Orientation};
use gtk::glib::types::StaticType;

use crate::{
    config,
    i18n::{tr, tr_args, trn, trn_args},
    image_io,
    logic::{
        QualityPreset, quality_percentage, human_bytes,
        is_supported_image,
    },
    processor::{self, ProcessOptions, ProcessResult},
};

#[derive(Debug, Clone)]
struct ImageEntry {
    path: PathBuf,
    width: u32,
    height: u32,
    size_bytes: u64,
    width_preset: u32,
    custom_width: u32,
    quality_preset: QualityPreset,
    custom_quality: Option<u8>,
}

#[derive(Debug, Clone)]
struct DecodedSourceCache {
    path: PathBuf,
    image: Arc<image_io::RawImage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewCacheKey {
    path: PathBuf,
    target_width: u32,
    quality_preset: QualityPreset,
    custom_quality: Option<u8>,
}

#[derive(Clone)]
struct PreviewRenderCache {
    key: PreviewCacheKey,
    texture: gtk::gdk::Texture,
    output_width: u32,
    output_height: u32,
    original_bytes: u64,
    encoded_size: u64,
}

#[derive(Clone)]
struct ActiveRenderCache {
    path: PathBuf,
    original_texture: Option<gtk::gdk::Texture>,
    preview: Option<PreviewRenderCache>,
}

#[derive(Default)]
struct AppState {
    entries: Vec<ImageEntry>,
    current_index: Option<usize>,
    bulk_selected: HashSet<PathBuf>,
    importing: bool,
    processing: bool,
    preview_generation: u64,
    preview_running: bool,
    preview_pending: bool,
    output_dir: Option<PathBuf>,
    decoded_source_cache: Option<DecodedSourceCache>,
    active_render_cache: Option<ActiveRenderCache>,
}

type SharedState = Rc<RefCell<AppState>>;

#[derive(Clone)]
struct CompareView {
    scroller: gtk::ScrolledWindow,
    root: gtk::AspectFrame,
    overlay: gtk::Overlay,
    picture: gtk::Picture,
    control: gtk::Overlay,
    handle: gtk::CenterBox,
    original: Rc<RefCell<Option<gtk::gdk::Texture>>>,
    preview: Rc<RefCell<Option<gtk::gdk::Texture>>>,
    position: Rc<Cell<f64>>,
    zoomed: Rc<Cell<bool>>,
}

/// True when a press at `x`/`y` (in `target` coordinates) lands on the Compare
/// handle. This is the arbitration point between moving the divider and panning
/// a zoomed image, so both gestures test it against the same allocation.
fn compare_point_on_handle(
    handle: &gtk::CenterBox,
    target: &gtk::Widget,
    x: f64,
    y: f64,
) -> bool {
    handle
        .compute_bounds(target)
        .map(|bounds| bounds.contains_point(&gtk::graphene::Point::new(x as f32, y as f32)))
        .unwrap_or(false)
}

const COMPARE_HANDLE_SIZE: i32 = 44;

fn compare_handle_width(width: i32) -> i32 {
    COMPARE_HANDLE_SIZE.min(width.max(1))
}

fn compare_control_travel(width: i32) -> i32 {
    (width.max(1) - compare_handle_width(width)).max(0)
}

fn compare_control_x(width: i32, position: f64) -> i32 {
    (f64::from(compare_control_travel(width)) * position.clamp(0.0, 1.0)).round() as i32
}

fn compare_split_fraction(width: i32, position: f64) -> f64 {
    let width = width.max(1);
    let control_width = compare_handle_width(width);
    let center_x =
        f64::from(compare_control_x(width, position)) + f64::from(control_width) / 2.0;
    (center_x / f64::from(width)).clamp(0.0, 1.0)
}

fn compare_diag_bounds(widget: &gtk::Widget, target: &gtk::Widget) -> String {
    match widget.compute_bounds(target) {
        Some(bounds) => format!(
            "x={:.1} y={:.1} w={:.1} h={:.1}",
            bounds.x(),
            bounds.y(),
            bounds.width(),
            bounds.height()
        ),
        None => "unavailable".to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn log_compare_geometry(
    phase: &str,
    root: &gtk::AspectFrame,
    overlay: &gtk::Overlay,
    picture: &gtk::Picture,
    control: &gtk::Overlay,
    divider: &gtk::Box,
    handle: &gtk::CenterBox,
    handle_icons: &gtk::Box,
    start_icon: &gtk::Image,
    end_icon: &gtk::Image,
    position: f64,
    start: Option<(f64, f64)>,
    offset: Option<(f64, f64)>,
) {
    let overlay_widget = overlay.upcast_ref::<gtk::Widget>();
    let handle_widget = handle.upcast_ref::<gtk::Widget>();
    let pointer_x = match (start, offset) {
        (Some((start_x, _)), Some((offset_x, _))) => Some(start_x + offset_x),
        (Some((start_x, _)), None) => Some(start_x),
        _ => None,
    };

    eprintln!(
        "[Image Bench CompareDiag] phase={phase} position={position:.6} start={start:?} offset={offset:?} pointer_x={pointer_x:?} root={}x{} overlay={}x{} picture={}x{}",
        root.width(),
        root.height(),
        overlay.width(),
        overlay.height(),
        picture.width(),
        picture.height(),
    );
    eprintln!(
        "[Image Bench CompareDiag] phase={phase} bounds_in_overlay root=[{}] picture=[{}] control=[{}] divider=[{}] handle=[{}]",
        compare_diag_bounds(root.upcast_ref::<gtk::Widget>(), overlay_widget),
        compare_diag_bounds(picture.upcast_ref::<gtk::Widget>(), overlay_widget),
        compare_diag_bounds(control.upcast_ref::<gtk::Widget>(), overlay_widget),
        compare_diag_bounds(divider.upcast_ref::<gtk::Widget>(), overlay_widget),
        compare_diag_bounds(handle_widget, overlay_widget),
    );
    eprintln!(
        "[Image Bench CompareDiag] phase={phase} handle_content icons=[{}] start_icon=[{}] end_icon=[{}]",
        compare_diag_bounds(handle_icons.upcast_ref::<gtk::Widget>(), handle_widget),
        compare_diag_bounds(start_icon.upcast_ref::<gtk::Widget>(), handle_widget),
        compare_diag_bounds(end_icon.upcast_ref::<gtk::Widget>(), handle_widget),
    );
}

fn install_compare_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        r#"
.image-bench-workspace,
.compare-divider {
    background-color: @window_bg_color;
}

.compare-handle {
    background-color: @window_bg_color;
    color: @window_fg_color;
    border-radius: 9999px;
}

.compare-handle-hover {
    background-color: @accent_bg_color;
    color: @accent_fg_color;
}

.compare-divider {
    border: none;
    box-shadow: none;
}


.image-bench-preview-modes toggle:checked {
    background-color: @accent_bg_color;
    color: @accent_fg_color;
}

.image-bench-info-target {
    background-color: transparent;
    color: @window_fg_color;
    border-radius: 9px;
    transition: background-color 160ms ease-out, color 160ms ease-out;
}

.image-bench-info-target-hover {
    background-color: alpha(@window_fg_color, 0.08);
    color: @accent_bg_color;
}

.image-bench-info-indicator {
    opacity: 0.65;
    transition: opacity 160ms ease-out;
}

.image-bench-info-target-hover .image-bench-info-indicator {
    opacity: 1;
}
"#,
    );

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

impl CompareView {
    fn new() -> Self {
        install_compare_css();

        // The AspectFrame owns the actual painted-image rectangle. Its child
        // overlay is therefore exactly the image bounds, so Compare geometry
        // no longer has to reconstruct GTK Picture's contained rectangle.
        let root = gtk::AspectFrame::new(0.5, 0.5, 1.0, false);
        root.set_hexpand(true);
        root.set_vexpand(true);

        // Same viewport construction as the single preview: at Fit the scroller
        // never scrolls, so the AspectFrame allocation stays exactly the painted
        // image and all existing divider geometry is unaffected.
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
        scroller.set_hexpand(true);
        scroller.set_vexpand(true);
        scroller.set_child(Some(&root));

        let overlay = gtk::Overlay::new();
        overlay.set_hexpand(true);
        overlay.set_vexpand(true);
        overlay.set_overflow(gtk::Overflow::Hidden);
        root.set_child(Some(&overlay));

        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Fill);
        picture.set_can_shrink(true);
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        overlay.set_child(Some(&picture));

        // One moving CompareControl owns both the divider and the drag handle.
        // The parent overlay positions only this control; its children never
        // calculate independent horizontal positions.
        let control = gtk::Overlay::new();
        control.set_visible(false);

        let control_fill = gtk::Box::new(Orientation::Vertical, 0);
        control_fill.set_hexpand(true);
        control_fill.set_vexpand(true);
        control_fill.set_can_target(false);
        control.set_child(Some(&control_fill));

        let divider = gtk::Box::new(Orientation::Vertical, 0);
        divider.add_css_class("compare-divider");
        divider.set_width_request(2);
        divider.set_halign(Align::Center);
        divider.set_valign(Align::Fill);
        divider.set_can_target(false);
        control.add_overlay(&divider);
        control.set_clip_overlay(&divider, true);

        // Pure visual drag handle: no GtkButton semantics or theme padding.
        // It is centered inside CompareControl, so its center and the divider
        // are physically tied to the same parent allocation.
        let handle = gtk::CenterBox::new();
        handle.set_size_request(COMPARE_HANDLE_SIZE, COMPARE_HANDLE_SIZE);
        handle.set_halign(Align::Center);
        handle.set_valign(Align::Center);
        handle.set_can_target(true);
        handle.add_css_class("compare-handle");
        handle.set_tooltip_text(Some(&tr("Drag to compare Original and Preview")));

        let handle_icons = gtk::Box::new(Orientation::Horizontal, 2);
        let start_icon = gtk::Image::from_icon_name("pan-start-symbolic");
        let end_icon = gtk::Image::from_icon_name("pan-end-symbolic");
        start_icon.set_pixel_size(16);
        end_icon.set_pixel_size(16);
        handle_icons.append(&start_icon);
        handle_icons.append(&end_icon);
        handle.set_center_widget(Some(&handle_icons));

        let handle_motion = gtk::EventControllerMotion::new();
        let handle_hover = handle.downgrade();
        handle_motion.connect_enter(move |_, _, _| {
            if let Some(handle) = handle_hover.upgrade() {
                handle.add_css_class("compare-handle-hover");
            }
        });
        let handle_leave = handle.downgrade();
        handle_motion.connect_leave(move |_| {
            if let Some(handle) = handle_leave.upgrade() {
                handle.remove_css_class("compare-handle-hover");
            }
        });
        handle.add_controller(handle_motion);

        control.add_overlay(&handle);
        control.set_clip_overlay(&handle, true);
        overlay.add_overlay(&control);
        overlay.set_clip_overlay(&control, true);

        let original: Rc<RefCell<Option<gtk::gdk::Texture>>> =
            Rc::new(RefCell::new(None));
        let preview: Rc<RefCell<Option<gtk::gdk::Texture>>> =
            Rc::new(RefCell::new(None));
        let position: Rc<Cell<f64>> = Rc::new(Cell::new(0.5_f64));

        {
            let control = control.downgrade();
            let position = position.clone();
            overlay.connect_get_child_position(move |overlay, widget| {
                let control = control.upgrade()?;
                if widget != control.upcast_ref::<gtk::Widget>() {
                    return None;
                }

                let width = overlay.width().max(1);
                let height = overlay.height().max(1);
                let control_width = compare_handle_width(width);
                let x = compare_control_x(width, position.get());
                Some(gtk::gdk::Rectangle::new(x, 0, control_width, height))
            });
        }

        // One gesture on the fixed overlay is the sole source of compare drag state.
        // The handle itself moves during the gesture, so measuring drag offsets on
        // the handle would create a moving coordinate space and visible jitter.
        let drag = gtk::GestureDrag::new();
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let drag_start = Rc::new(Cell::new(0.5_f64));
        let drag_origin = Rc::new(Cell::new(None::<(f64, f64)>));
        let zoomed: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        // Whether the current sequence belongs to the divider. At Fit a drag
        // anywhere still moves the divider; once zoomed, only the handle does,
        // so the rest of the image is free for panning.
        let drag_active = Rc::new(Cell::new(true));

        {
            let root = root.downgrade();
            let overlay = overlay.downgrade();
            let picture = picture.downgrade();
            let control = control.downgrade();
            let divider = divider.downgrade();
            let handle = handle.downgrade();
            let handle_icons = handle_icons.downgrade();
            let start_icon = start_icon.downgrade();
            let end_icon = end_icon.downgrade();
            let position = position.clone();
            let drag_start = drag_start.clone();
            let drag_origin = drag_origin.clone();
            let zoomed_begin = zoomed.clone();
            let drag_active_begin = drag_active.clone();
            let handle_hit = handle.clone();
            let overlay_hit = overlay.clone();
            drag.connect_drag_begin(move |gesture, start_x, start_y| {
                let owns_sequence = if zoomed_begin.get() {
                    match (handle_hit.upgrade(), overlay_hit.upgrade()) {
                        (Some(handle), Some(overlay)) => compare_point_on_handle(
                            &handle,
                            overlay.upcast_ref::<gtk::Widget>(),
                            start_x,
                            start_y,
                        ),
                        _ => false,
                    }
                } else {
                    true
                };
                drag_active_begin.set(owns_sequence);
                if !owns_sequence {
                    // Leave the sequence unclaimed so the pan gesture can take it.
                    gesture.set_state(gtk::EventSequenceState::Denied);
                    return;
                }
                gesture.set_state(gtk::EventSequenceState::Claimed);
                drag_start.set(position.get());
                let origin = gesture.start_point().or(Some((start_x, start_y)));
                drag_origin.set(origin);
                let (Some(root), Some(overlay), Some(picture), Some(control), Some(divider), Some(handle), Some(handle_icons), Some(start_icon), Some(end_icon)) = (
                    root.upgrade(),
                    overlay.upgrade(),
                    picture.upgrade(),
                    control.upgrade(),
                    divider.upgrade(),
                    handle.upgrade(),
                    handle_icons.upgrade(),
                    start_icon.upgrade(),
                    end_icon.upgrade(),
                ) else {
                    return;
                };
                log_compare_geometry(
                    "drag-begin",
                    &root,
                    &overlay,
                    &picture,
                    &control,
                    &divider,
                    &handle,
                    &handle_icons,
                    &start_icon,
                    &end_icon,
                    position.get(),
                    origin,
                    None,
                );
            });
        }

        {
            let overlay = overlay.downgrade();
            let picture = picture.downgrade();
            let original = original.clone();
            let preview = preview.clone();
            let position = position.clone();
            let drag_start = drag_start.clone();
            let drag_active_update = drag_active.clone();
            drag.connect_drag_update(move |_, offset_x, _| {
                if !drag_active_update.get() { return; }
                let Some(overlay) = overlay.upgrade() else { return; };
                let Some(picture) = picture.upgrade() else { return; };
                let original_ref = original.borrow();
                if original_ref.is_none() { return; }
                let width = overlay.width().max(1);
                let travel = f64::from(compare_control_travel(width).max(1));
                let value = (drag_start.get() + offset_x / travel).clamp(0.0, 1.0);
                position.set(value);
                drop(original_ref);
                redraw_compare_parts(&picture, &original, &preview, value);
                overlay.queue_allocate();
            });
        }

        {
            let root = root.downgrade();
            let overlay = overlay.downgrade();
            let picture = picture.downgrade();
            let control = control.downgrade();
            let divider = divider.downgrade();
            let handle = handle.downgrade();
            let handle_icons = handle_icons.downgrade();
            let start_icon = start_icon.downgrade();
            let end_icon = end_icon.downgrade();
            let position = position.clone();
            let drag_origin = drag_origin.clone();
            let drag_active_end = drag_active.clone();
            drag.connect_drag_end(move |_, offset_x, offset_y| {
                if !drag_active_end.get() { return; }
                let (Some(root), Some(overlay), Some(picture), Some(control), Some(divider), Some(handle), Some(handle_icons), Some(start_icon), Some(end_icon)) = (
                    root.upgrade(),
                    overlay.upgrade(),
                    picture.upgrade(),
                    control.upgrade(),
                    divider.upgrade(),
                    handle.upgrade(),
                    handle_icons.upgrade(),
                    start_icon.upgrade(),
                    end_icon.upgrade(),
                ) else {
                    return;
                };
                log_compare_geometry(
                    "drag-end",
                    &root,
                    &overlay,
                    &picture,
                    &control,
                    &divider,
                    &handle,
                    &handle_icons,
                    &start_icon,
                    &end_icon,
                    position.get(),
                    drag_origin.get(),
                    Some((offset_x, offset_y)),
                );
                drag_origin.set(None);
            });
        }

        overlay.add_controller(drag);

        Self {
            scroller,
            root,
            overlay,
            picture,
            control,
            handle,
            original,
            preview,
            position,
            zoomed,
        }
    }

    fn set_textures(&self, original: gtk::gdk::Texture, preview: gtk::gdk::Texture) {
        let ratio = original.width().max(1) as f32 / original.height().max(1) as f32;
        eprintln!(
            "[Image Bench CompareDiag] phase=set-textures original={}x{} preview={}x{} aspect_ratio={ratio:.6}",
            original.width(),
            original.height(),
            preview.width(),
            preview.height(),
        );
        self.root.set_ratio(ratio);
        *self.original.borrow_mut() = Some(original);
        *self.preview.borrow_mut() = Some(preview);
        self.position.set(0.5);
        self.control.set_visible(true);
        redraw_compare(self);
    }

    fn clear(&self) {
        *self.original.borrow_mut() = None;
        *self.preview.borrow_mut() = None;
        self.control.set_visible(false);
        self.picture.set_paintable(None::<&gtk::gdk::Paintable>);
    }
}

fn redraw_compare(view: &CompareView) {
    redraw_compare_parts(
        &view.picture,
        &view.original,
        &view.preview,
        view.position.get(),
    );
    view.overlay.queue_allocate();
}

fn redraw_compare_parts(
    picture: &gtk::Picture,
    original: &Rc<RefCell<Option<gtk::gdk::Texture>>>,
    preview: &Rc<RefCell<Option<gtk::gdk::Texture>>>,
    position: f64,
) {
    let original = original.borrow();
    let preview = preview.borrow();
    let (Some(original), Some(preview)) = (original.as_ref(), preview.as_ref()) else {
        picture.set_paintable(None::<&gtk::gdk::Paintable>);
        return;
    };

    let width = original.width().max(1) as f32;
    let height = original.height().max(1) as f32;
    let bounds = gtk::graphene::Rect::new(0.0, 0.0, width, height);
    let split_fraction = compare_split_fraction(picture.width().max(1), position);
    let split_x = width * split_fraction as f32;

    let snapshot = gtk::Snapshot::new();

    // Preview fills the full canvas. Original is clipped on the left, so the
    // divider always reads as Original | Preview.
    snapshot.append_texture(preview, &bounds);
    let clip = gtk::graphene::Rect::new(0.0, 0.0, split_x, height);
    snapshot.push_clip(&clip);
    snapshot.append_texture(original, &bounds);
    snapshot.pop();


    let size = gtk::graphene::Size::new(width, height);
    if let Some(paintable) = snapshot.to_paintable(Some(&size)) {
        picture.set_paintable(Some(&paintable));
    }
}

#[derive(Clone)]
struct ContentUi {
    sidebar_button: gtk::ToggleButton,
    queue_button: gtk::ToggleButton,
    queue_split: adw::OverlaySplitView,
    preview_modes: adw::ToggleGroup,
    stack: gtk::Stack,
    preview_display: gtk::Stack,
    preview_spinner: gtk::Spinner,
    preview_scroller: gtk::ScrolledWindow,
    preview: gtk::Picture,
    zoom_row: gtk::DropDown,
    pointer: Rc<Cell<(f64, f64)>>,
    zoom_anchor: Rc<Cell<Option<(f64, f64, f64, f64)>>>,
    compare: CompareView,
    preview_title: gtk::Label,
    preview_meta: gtk::Label,
    queue_count_label: gtk::Label,
    listbox: gtk::ListBox,
    queue_settings_labels: Rc<RefCell<Vec<gtk::Label>>>,
    progress_box: gtk::Box,
    progress_label: gtk::Label,
    progress: gtk::ProgressBar,
}

/// Shared zoom state for the central single-image preview.
///
/// `Fit` is GTK's own contained layout; every other level paints the image at
/// an exact multiple of its intrinsic pixel size inside the preview scroller.
/// Compare keeps its own AspectFrame geometry and is therefore excluded from
/// zoom in this gate; see `refresh_zoom_state`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ZoomLevel {
    Fit,
    Scale(f64),
}

const ZOOM_SCALES: [f64; 3] = [1.0, 2.0, 4.0];

fn zoom_level_from_index(index: u32) -> ZoomLevel {
    match index.checked_sub(1) {
        Some(step) => ZOOM_SCALES
            .get(step as usize)
            .copied()
            .map(ZoomLevel::Scale)
            .unwrap_or(ZoomLevel::Fit),
        None => ZoomLevel::Fit,
    }
}

fn apply_fit_layout(content: &ContentUi) {
    content
        .preview_scroller
        .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    content.preview.set_size_request(-1, -1);
    content.preview.set_content_fit(gtk::ContentFit::Contain);
    content.preview.set_can_shrink(true);
    content.preview.set_halign(Align::Fill);
    content.preview.set_valign(Align::Fill);
    content.preview.set_hexpand(true);
    content.preview.set_vexpand(true);
    content.preview_scroller.set_cursor_from_name(None);

    content
        .compare
        .scroller
        .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    content.compare.root.set_size_request(-1, -1);
    content.compare.root.set_halign(Align::Fill);
    content.compare.root.set_valign(Align::Fill);
    content.compare.root.set_hexpand(true);
    content.compare.root.set_vexpand(true);
    content.compare.scroller.set_cursor_from_name(None);
    content.compare.zoomed.set(false);
    content.zoom_anchor.set(None);
}

/// Returns the intrinsic pixel size of a picture's current paintable.
fn paintable_size(picture: &gtk::Picture) -> Option<(i32, i32)> {
    picture
        .paintable()
        .map(|paintable| (paintable.intrinsic_width(), paintable.intrinsic_height()))
        .filter(|(width, height)| *width > 0 && *height > 0)
}

fn scaled_request(width: i32, height: i32, scale: f64) -> (i32, i32) {
    (
        ((f64::from(width) * scale).round() as i32).max(1),
        ((f64::from(height) * scale).round() as i32).max(1),
    )
}

fn compare_is_visible(content: &ContentUi) -> bool {
    content.preview_display.visible_child_name().as_deref() == Some("compare")
}

/// The painted image rectangle of the visible view, in viewport coordinates.
///
/// For Compare the overlay already is the image rectangle. For the single view
/// the picture allocation may be letterboxed at Fit, so the contained rectangle
/// is derived; when zoomed the allocation matches the image aspect exactly and
/// the same formula yields a zero offset.
fn visible_image_rect(content: &ContentUi) -> Option<(gtk::ScrolledWindow, f64, f64, f64, f64)> {
    if compare_is_visible(content) {
        let scroller = content.compare.scroller.clone();
        let bounds = content.compare.overlay.compute_bounds(&scroller)?;
        return Some((
            scroller,
            f64::from(bounds.x()),
            f64::from(bounds.y()),
            f64::from(bounds.width()),
            f64::from(bounds.height()),
        ));
    }

    let scroller = content.preview_scroller.clone();
    let bounds = content.preview.compute_bounds(&scroller)?;
    let (intrinsic_width, intrinsic_height) = paintable_size(&content.preview)?;
    let allocated_width = f64::from(bounds.width());
    let allocated_height = f64::from(bounds.height());
    let contain = (allocated_width / f64::from(intrinsic_width))
        .min(allocated_height / f64::from(intrinsic_height));
    let painted_width = f64::from(intrinsic_width) * contain;
    let painted_height = f64::from(intrinsic_height) * contain;
    Some((
        scroller,
        f64::from(bounds.x()) + (allocated_width - painted_width) / 2.0,
        f64::from(bounds.y()) + (allocated_height - painted_height) / 2.0,
        painted_width,
        painted_height,
    ))
}

/// Records the point that must stay put across a zoom change.
///
/// `pointer` is in preview-stack coordinates; `None` anchors on the centre of
/// the viewport, which is what the dropdown should do.
fn capture_zoom_anchor(content: &ContentUi, pointer: Option<(f64, f64)>) {
    let Some((scroller, rect_x, rect_y, rect_width, rect_height)) = visible_image_rect(content)
    else {
        content.zoom_anchor.set(None);
        return;
    };
    if rect_width <= 0.0 || rect_height <= 0.0 {
        content.zoom_anchor.set(None);
        return;
    }

    let centre = (
        f64::from(scroller.width()) / 2.0,
        f64::from(scroller.height()) / 2.0,
    );
    let (viewport_x, viewport_y) = match pointer {
        Some((x, y)) => {
            let point = gtk::graphene::Point::new(x as f32, y as f32);
            match content.preview_display.compute_point(&scroller, &point) {
                Some(mapped) => (f64::from(mapped.x()), f64::from(mapped.y())),
                None => centre,
            }
        }
        None => centre,
    };

    // Outside the image, fall back to the centre so zoom never jumps off-image.
    let inside = viewport_x >= rect_x
        && viewport_x <= rect_x + rect_width
        && viewport_y >= rect_y
        && viewport_y <= rect_y + rect_height;
    let (viewport_x, viewport_y) = if inside {
        (viewport_x, viewport_y)
    } else {
        centre
    };

    content.zoom_anchor.set(Some((
        ((viewport_x - rect_x) / rect_width).clamp(0.0, 1.0),
        ((viewport_y - rect_y) / rect_height).clamp(0.0, 1.0),
        viewport_x,
        viewport_y,
    )));
}

/// Puts the anchored image point back under the same viewport position.
///
/// The adjustments only accept the new values once the larger child has been
/// allocated, so this runs on the next main-loop iteration.
fn restore_zoom_anchor(
    content: &ContentUi,
    scroller: &gtk::ScrolledWindow,
    content_width: f64,
    content_height: f64,
) {
    let Some((normalized_x, normalized_y, viewport_x, viewport_y)) = content.zoom_anchor.take()
    else {
        return;
    };
    let scroller = scroller.clone();
    glib::idle_add_local_once(move || {
        scroller
            .hadjustment()
            .set_value(normalized_x * content_width - viewport_x);
        scroller
            .vadjustment()
            .set_value(normalized_y * content_height - viewport_y);
    });
}

/// Re-applies the selected zoom level to the current preview paintable.
///
/// Called both when the level changes and whenever a picture receives a new
/// paintable, so switching Original/Compare/Preview or re-rendering keeps zoom.
fn apply_preview_zoom(content: &ContentUi) {
    let ZoomLevel::Scale(scale) = zoom_level_from_index(content.zoom_row.selected()) else {
        apply_fit_layout(content);
        return;
    };
    let compare_mode = compare_is_visible(content);

    // Single view.
    if let Some((width, height)) = paintable_size(&content.preview) {
        let (request_width, request_height) = scaled_request(width, height, scale);
        content
            .preview_scroller
            .set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        content.preview.set_content_fit(gtk::ContentFit::Fill);
        content.preview.set_can_shrink(false);
        content.preview.set_hexpand(false);
        content.preview.set_vexpand(false);
        content.preview.set_halign(Align::Center);
        content.preview.set_valign(Align::Center);
        content
            .preview
            .set_size_request(request_width, request_height);
        if !compare_mode {
            restore_zoom_anchor(
                content,
                &content.preview_scroller,
                f64::from(request_width),
                f64::from(request_height),
            );
        }
    }

    // Compare view. The AspectFrame carries the request, so its child overlay
    // still equals the painted image and the divider geometry is unchanged.
    if let Some((width, height)) = paintable_size(&content.compare.picture) {
        let (request_width, request_height) = scaled_request(width, height, scale);
        content
            .compare
            .scroller
            .set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        content.compare.root.set_hexpand(false);
        content.compare.root.set_vexpand(false);
        content.compare.root.set_halign(Align::Center);
        content.compare.root.set_valign(Align::Center);
        content
            .compare
            .root
            .set_size_request(request_width, request_height);
        content.compare.zoomed.set(true);
        if compare_mode {
            restore_zoom_anchor(
                content,
                &content.compare.scroller,
                f64::from(request_width),
                f64::from(request_height),
            );
        }
    }

    update_pan_cursor(content, false);
}

/// Shows the grab affordance only when there is actually something to pan.
fn update_pan_cursor(content: &ContentUi, dragging: bool) {
    let zoomed = !matches!(
        zoom_level_from_index(content.zoom_row.selected()),
        ZoomLevel::Fit
    );
    let cursor = if !zoomed {
        None
    } else if dragging {
        Some("grabbing")
    } else {
        Some("grab")
    };
    content.preview_scroller.set_cursor_from_name(cursor);
    content.compare.scroller.set_cursor_from_name(cursor);
}

/// Installs primary-button pan on one preview viewport.
///
/// Pan drives the scroller's own adjustments, so clamping at the image edges is
/// GTK's, not ours. `handle` is the Compare drag handle when this viewport has
/// one; a press on it belongs to the divider, never to pan.
fn connect_viewport_pan(
    content: &ContentUi,
    scroller: &gtk::ScrolledWindow,
    handle: Option<gtk::CenterBox>,
) {
    let pan = gtk::GestureDrag::new();
    pan.set_button(gtk::gdk::BUTTON_PRIMARY);
    // Capture phase: GtkScrolledWindow installs its own gestures, so a bubble
    // phase drag can be claimed before it reaches us.
    pan.set_propagation_phase(gtk::PropagationPhase::Capture);
    let pan_origin = Rc::new(Cell::new((0.0f64, 0.0f64)));
    let pan_active = Rc::new(Cell::new(false));

    let content_begin = content.clone();
    let scroller_begin = scroller.clone();
    let origin_begin = pan_origin.clone();
    let active_begin = pan_active.clone();
    let handle_begin = handle.clone();
    pan.connect_drag_begin(move |gesture, start_x, start_y| {
        let zoomed = !matches!(
            zoom_level_from_index(content_begin.zoom_row.selected()),
            ZoomLevel::Fit
        );
        let on_handle = handle_begin.as_ref().is_some_and(|handle| {
            compare_point_on_handle(
                handle,
                scroller_begin.upcast_ref::<gtk::Widget>(),
                start_x,
                start_y,
            )
        });
        let owns_sequence = zoomed && !on_handle;
        active_begin.set(owns_sequence);
        if !owns_sequence {
            return;
        }
        gesture.set_state(gtk::EventSequenceState::Claimed);
        origin_begin.set((
            scroller_begin.hadjustment().value(),
            scroller_begin.vadjustment().value(),
        ));
        update_pan_cursor(&content_begin, true);
    });

    let scroller_update = scroller.clone();
    let origin_update = pan_origin.clone();
    let active_update = pan_active.clone();
    pan.connect_drag_update(move |_, offset_x, offset_y| {
        if !active_update.get() {
            return;
        }
        let (start_h, start_v) = origin_update.get();
        // Dragging right moves the image right, so the viewport moves left.
        scroller_update.hadjustment().set_value(start_h - offset_x);
        scroller_update.vadjustment().set_value(start_v - offset_y);
    });

    let content_end = content.clone();
    let active_end = pan_active.clone();
    pan.connect_drag_end(move |_, _, _| {
        if !active_end.get() {
            return;
        }
        active_end.set(false);
        update_pan_cursor(&content_end, false);
    });
    scroller.add_controller(pan);
}

/// Installs pan on both viewports and one shared scroll-wheel zoom.
fn connect_preview_navigation(content: &ContentUi) {
    connect_viewport_pan(content, &content.preview_scroller, None);
    connect_viewport_pan(
        content,
        &content.compare.scroller,
        Some(content.compare.handle.clone()),
    );

    // Last pointer position over the preview stack, used to anchor wheel zoom.
    let motion = gtk::EventControllerMotion::new();
    let pointer_motion = content.pointer.clone();
    motion.connect_motion(move |_, x, y| {
        pointer_motion.set((x, y));
    });
    content.preview_display.add_controller(motion);

    // The controller lives on the Stack, the common ancestor of both viewports,
    // so one wheel handler serves Original, Compare and Preview.
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
    // Touchpads emit many small deltas; accumulate so one notch is one step.
    let scroll_delta = Rc::new(Cell::new(0.0f64));
    let content_scroll = content.clone();
    scroll.connect_scroll(move |_, _, delta_y| {
        if !content_scroll.zoom_row.is_sensitive() {
            return glib::Propagation::Proceed;
        }

        let accumulated = scroll_delta.get() + delta_y;
        if accumulated.abs() < 1.0 {
            scroll_delta.set(accumulated);
            return glib::Propagation::Stop;
        }
        scroll_delta.set(0.0);

        let steps = u32::try_from(ZOOM_SCALES.len()).unwrap_or(0);
        let selected = content_scroll.zoom_row.selected();
        let next = if accumulated < 0.0 {
            selected.saturating_add(1).min(steps)
        } else {
            selected.saturating_sub(1)
        };
        if next != selected {
            // Measure before the level changes; the old rectangle defines where
            // the cursor currently sits in the image.
            capture_zoom_anchor(&content_scroll, Some(content_scroll.pointer.get()));
            content_scroll.zoom_row.set_selected(next);
        }
        glib::Propagation::Stop
    });
    content.preview_display.add_controller(scroll);
}

/// Keeps the zoom control consistent with the queue state.
fn refresh_zoom_state(content: &ContentUi) {
    content
        .zoom_row
        .set_sensitive(content.preview_modes.is_sensitive());
    apply_preview_zoom(content);
}

#[derive(Clone)]
struct SidebarUi {
    toolbar: adw::ToolbarView,
    add_images_button: gtk::Button,
    add_folder_button: gtk::Button,
    clear_button: gtk::Button,
    queue_row: adw::ActionRow,
    width_row: adw::ComboRow,
    custom_width_row: adw::SpinRow,
    quality_row: adw::ComboRow,
    custom_quality_row: adw::SpinRow,
    apply_quality_row: adw::PreferencesRow,
    apply_quality_button: gtk::Button,
    applied_custom_quality: Rc<Cell<u8>>,
    custom_quality_dirty: Rc<Cell<bool>>,
    syncing_quality_ui: Rc<Cell<bool>>,
    syncing_resize_ui: Rc<Cell<bool>>,
    apply_settings_menu: gtk::MenuButton,
    apply_selected_settings_button: gtk::Button,
    apply_all_settings_button: gtk::Button,
    output_row: adw::ActionRow,
    choose_output_button: gtk::Button,
    filename_suffix_check: gtk::CheckButton,
    filename_suffix_entry: adw::EntryRow,
    quality_suffix_row: adw::ActionRow,
    quality_suffix_check: gtk::CheckButton,
    optimize_button: gtk::Button,
}

pub fn build_window(app: &adw::Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Image Bench")
        .default_width(1120)
        .default_height(760)
        .build();
    window.set_size_request(520, 520);

    let toast_overlay = adw::ToastOverlay::new();
    window.set_content(Some(&toast_overlay));

    let split_view = adw::OverlaySplitView::new();
    split_view.set_sidebar_width_fraction(0.34);
    split_view.set_min_sidebar_width(300.0);
    split_view.set_max_sidebar_width(390.0);
    toast_overlay.set_child(Some(&split_view));

    let sidebar = build_sidebar();
    split_view.set_sidebar(Some(&sidebar.toolbar));

    let content = build_content();
    split_view.set_content(Some(&content.queue_split));

    split_view
        .bind_property("show-sidebar", &content.sidebar_button, "active")
        .bidirectional()
        .sync_create()
        .build();

    content
        .queue_split
        .bind_property("show-sidebar", &content.queue_button, "active")
        .bidirectional()
        .sync_create()
        .build();

    let condition = adw::BreakpointCondition::parse("max-width: 760sp")
        .expect("valid Image Bench breakpoint");
    let breakpoint = adw::Breakpoint::new(condition);
    let split_view_apply = split_view.clone();
    breakpoint.connect_apply(move |_| {
        split_view_apply.set_collapsed(true);
    });
    let split_view_unapply = split_view.clone();
    breakpoint.connect_unapply(move |_| {
        split_view_unapply.set_collapsed(false);
    });
    window.add_breakpoint(breakpoint);

    let queue_condition = adw::BreakpointCondition::parse("max-width: 1050sp")
        .expect("valid Image Bench queue breakpoint");
    let queue_breakpoint = adw::Breakpoint::new(queue_condition);
    let queue_split_apply = content.queue_split.clone();
    queue_breakpoint.connect_apply(move |_| {
        queue_split_apply.set_collapsed(true);
        queue_split_apply.set_show_sidebar(false);
    });
    let queue_split_unapply = content.queue_split.clone();
    let queue_button_unapply = content.queue_button.clone();
    queue_breakpoint.connect_unapply(move |_| {
        queue_split_unapply.set_collapsed(false);
        queue_split_unapply.set_show_sidebar(queue_button_unapply.is_sensitive());
    });
    window.add_breakpoint(queue_breakpoint);

    let state = Rc::new(RefCell::new(AppState::default()));
    connect_basic_settings(&sidebar, &content, &state);
    connect_queue_state(&sidebar, &content, &state);
    connect_import(&window, &sidebar, &content, &state);
    connect_output_folder(&window, &sidebar, &content, &state);
    connect_optimize(&toast_overlay, &sidebar, &content, &state);
    refresh_queue(&sidebar, &content, &state);

    // Keep the passive widgets alive through the GTK widget tree. Signal wiring is
    // introduced feature-by-feature in the following migration gates.
    let _ = (
        sidebar.add_images_button,
        sidebar.add_folder_button,
        sidebar.clear_button,
        sidebar.width_row,
        sidebar.custom_width_row,
        sidebar.quality_row,
        sidebar.custom_quality_row,
        sidebar.output_row,
        sidebar.choose_output_button,
        sidebar.filename_suffix_check,
        sidebar.filename_suffix_entry,
        sidebar.quality_suffix_row,
        sidebar.quality_suffix_check,
        sidebar.optimize_button,
        content.sidebar_button,
        content.queue_button,
        content.queue_split,
        content.stack,
        content.preview_display,
        content.preview_spinner,
        content.preview_scroller,
        content.preview,
        content.zoom_row,
        content.compare,
        content.preview_title,
        content.preview_meta,
        content.queue_count_label,
        content.listbox,
        content.queue_settings_labels,
        content.progress_box,
        content.progress_label,
        content.progress,
    );

    window
}


fn image_filter() -> (gtk::FileFilter, gio::ListStore) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&tr("JPEG and PNG images")));
    for pattern in ["*.jpg", "*.jpeg", "*.JPG", "*.JPEG", "*.png", "*.PNG"] {
        filter.add_pattern(pattern);
    }

    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    (filter, filters)
}

fn paths_from_model(model: &gio::ListModel) -> Vec<PathBuf> {
    (0..model.n_items())
        .filter_map(|index| model.item(index))
        .filter_map(|item| item.downcast::<gio::File>().ok())
        .filter_map(|file| file.path())
        .collect()
}

fn spawn_add_paths(
    paths: Vec<PathBuf>,
    sidebar: SidebarUi,
    content: ContentUi,
    state: SharedState,
) {
    glib::MainContext::default().spawn_local(async move {
        add_paths(paths, &sidebar, &content, &state).await;
    });
}

fn connect_import(
    window: &adw::ApplicationWindow,
    sidebar: &SidebarUi,
    content: &ContentUi,
    state: &SharedState,
) {
    let add_images_button = sidebar.add_images_button.clone();
    let window_for_files = window.clone();
    let sidebar_for_files = sidebar.clone();
    let content_for_files = content.clone();
    let state_for_files = state.clone();
    add_images_button.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::builder()
            .title(tr("Add Images"))
            .modal(true)
            .build();
        let (filter, filters) = image_filter();
        dialog.set_filters(Some(&filters));
        dialog.set_default_filter(Some(&filter));

        let window = window_for_files.clone();
        let sidebar = sidebar_for_files.clone();
        let content = content_for_files.clone();
        let state = state_for_files.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(model) = dialog.open_multiple_future(Some(&window)).await else {
                return;
            };
            spawn_add_paths(paths_from_model(&model), sidebar, content, state);
        });
    });

    let add_folder_button = sidebar.add_folder_button.clone();
    let window_for_folder = window.clone();
    let sidebar_for_folder = sidebar.clone();
    let content_for_folder = content.clone();
    let state_for_folder = state.clone();
    add_folder_button.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::builder()
            .title(tr("Add Folder"))
            .modal(true)
            .build();
        let window = window_for_folder.clone();
        let sidebar = sidebar_for_folder.clone();
        let content = content_for_folder.clone();
        let state = state_for_folder.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(folder) = dialog.select_folder_future(Some(&window)).await else {
                return;
            };
            let Some(folder_path) = folder.path() else {
                return;
            };
            let mut paths = match fs::read_dir(&folder_path) {
                Ok(entries) => entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| is_supported_image(path))
                    .collect::<Vec<_>>(),
                Err(error) => {
                    eprintln!(
                        "[Image Bench] Could not read folder {}: {error}",
                        folder_path.display()
                    );
                    return;
                }
            };
            paths.sort();
            spawn_add_paths(paths, sidebar, content, state);
        });
    });

    let drop_target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );
    let sidebar_for_drop = sidebar.clone();
    let content_for_drop = content.clone();
    let state_for_drop = state.clone();
    drop_target.connect_drop(move |_, value, _, _| {
        let Ok(file_list) = value.get::<gtk::gdk::FileList>() else {
            return false;
        };
        let paths = file_list
            .files()
            .into_iter()
            .filter_map(|file| file.path())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return false;
        }
        spawn_add_paths(
            paths,
            sidebar_for_drop.clone(),
            content_for_drop.clone(),
            state_for_drop.clone(),
        );
        true
    });
    content.queue_split.add_controller(drop_target);
}

async fn add_paths(
    paths: Vec<PathBuf>,
    sidebar: &SidebarUi,
    content: &ContentUi,
    state: &SharedState,
) {
    if paths.is_empty() || state.borrow().importing {
        return;
    }

    let mut existing = state
        .borrow()
        .entries
        .iter()
        .map(|entry| entry.path.canonicalize().unwrap_or_else(|_| entry.path.clone()))
        .collect::<HashSet<_>>();
    let mut candidates = Vec::new();
    let mut skipped = 0usize;

    for path in paths {
        if !is_supported_image(&path) {
            skipped += 1;
            continue;
        }
        let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !existing.insert(resolved) {
            skipped += 1;
            continue;
        }
        candidates.push(path);
    }

    if candidates.is_empty() {
        if skipped > 0 {
            eprintln!("[Image Bench] Skipped {skipped} unsupported or duplicate item(s)");
        }
        return;
    }

    state.borrow_mut().importing = true;
    refresh_queue(sidebar, content, state);

    let mut imported = Vec::new();
    let mut first_decoded_cache = None;
    for path in candidates {
        match image_io::decode(&path, None).await {
            Ok(decoded) => match fs::metadata(&path) {
                Ok(metadata) => {
                    let decoded = Arc::new(decoded);
                    if first_decoded_cache.is_none() {
                        first_decoded_cache = Some(DecodedSourceCache {
                            path: path.clone(),
                            image: decoded.clone(),
                        });
                    }
                    imported.push(ImageEntry {
                        path,
                        width: decoded.width,
                        height: decoded.height,
                        size_bytes: metadata.len(),
                        width_preset: 1,
                        custom_width: 1280,
                        quality_preset: QualityPreset::Balanced,
                        custom_quality: None,
                    });
                }
                Err(error) => {
                    skipped += 1;
                    eprintln!(
                        "[Image Bench] Could not read metadata for {}: {error}",
                        path.display()
                    );
                }
            },
            Err(error) => {
                skipped += 1;
                eprintln!("[Image Bench] Could not inspect {}: {error}", path.display());
            }
        }
    }

    let first_new_index = {
        let mut app_state = state.borrow_mut();
        let first_new_index = app_state.entries.len();
        app_state.entries.extend(imported);
        if first_new_index < app_state.entries.len() {
            app_state.current_index = Some(first_new_index);
        }
        app_state.decoded_source_cache = first_decoded_cache;
        app_state.importing = false;
        first_new_index
    };
    refresh_queue(sidebar, content, state);

    if let Some(row) = content.listbox.row_at_index(first_new_index as i32) {
        content.listbox.select_row(Some(&row));
    }

    if skipped > 0 {
        eprintln!("[Image Bench] Skipped {skipped} unsupported, duplicate, or unreadable item(s)");
    }
}


fn entry_target_width(entry: &ImageEntry) -> u32 {
    match entry.width_preset {
        0 => 1920,
        1 => 1280,
        2 => 800,
        _ => entry.custom_width,
    }
}

fn entry_settings_text(entry: &ImageEntry) -> String {
    let resize = entry_target_width(entry);
    let quality = quality_percentage(entry.quality_preset, entry.custom_quality).unwrap_or(85);
    tr_args(
        "Resize: {resize} px  •  Quality: {quality}%",
        &[
            ("resize", resize.to_string()),
            ("quality", quality.to_string()),
        ],
    )
}

fn refresh_queue_settings_labels(content: &ContentUi, state: &SharedState) {
    let app_state = state.borrow();
    let labels = content.queue_settings_labels.borrow();
    for (index, label) in labels.iter().enumerate() {
        if let Some(entry) = app_state.entries.get(index) {
            label.set_text(&entry_settings_text(entry));
        }
    }
}

fn sync_resize_ui(entry: &ImageEntry, sidebar: &SidebarUi) {
    sidebar.syncing_resize_ui.set(true);
    sidebar.width_row.set_selected(entry.width_preset);
    sidebar.custom_width_row.set_value(f64::from(entry.custom_width));
    sidebar.custom_width_row.set_visible(entry.width_preset == 3);
    sidebar.syncing_resize_ui.set(false);
}

fn quality_preset_index(preset: QualityPreset) -> u32 {
    match preset {
        QualityPreset::High => 0,
        QualityPreset::Balanced => 1,
        QualityPreset::Compact => 2,
        QualityPreset::Custom => 3,
    }
}

fn quality_preset_from_index(index: u32) -> QualityPreset {
    match index {
        0 => QualityPreset::High,
        1 => QualityPreset::Balanced,
        2 => QualityPreset::Compact,
        _ => QualityPreset::Custom,
    }
}

fn sync_quality_ui(entry: &ImageEntry, sidebar: &SidebarUi) {
    sidebar.syncing_quality_ui.set(true);

    let is_custom = entry.quality_preset == QualityPreset::Custom;
    let quality = quality_percentage(entry.quality_preset, entry.custom_quality).unwrap_or(85);
    sidebar.quality_row.set_selected(quality_preset_index(entry.quality_preset));
    sidebar.applied_custom_quality.set(quality);
    sidebar.custom_quality_row.set_value(f64::from(quality));
    sidebar.custom_quality_row.set_visible(is_custom);
    sidebar.apply_quality_row.set_visible(is_custom);
    sidebar.custom_quality_dirty.set(false);
    sidebar.apply_quality_button.set_sensitive(false);
    update_quality_suffix_subtitle(&sidebar.quality_row, quality, &sidebar.quality_suffix_row);

    sidebar.syncing_quality_ui.set(false);
}

fn set_current_entry_resize(
    state: &SharedState,
    width_preset: u32,
    custom_width: u32,
) -> bool {
    let mut app_state = state.borrow_mut();
    let Some(index) = app_state.current_index else {
        return false;
    };
    let Some(entry) = app_state.entries.get_mut(index) else {
        return false;
    };
    entry.width_preset = width_preset;
    entry.custom_width = custom_width;
    true
}

fn set_current_entry_quality(
    state: &SharedState,
    preset: QualityPreset,
    custom_quality: Option<u8>,
) -> bool {
    let mut app_state = state.borrow_mut();
    let Some(index) = app_state.current_index else {
        return false;
    };
    let Some(entry) = app_state.entries.get_mut(index) else {
        return false;
    };
    entry.quality_preset = preset;
    entry.custom_quality = custom_quality;
    true
}

fn apply_current_settings_to_indices(state: &SharedState, target_indices: &[usize]) -> usize {
    let mut app_state = state.borrow_mut();
    let Some(current_index) = app_state.current_index else {
        return 0;
    };
    let Some(current) = app_state.entries.get(current_index) else {
        return 0;
    };
    let settings = (
        current.width_preset,
        current.custom_width,
        current.quality_preset,
        current.custom_quality,
    );

    let mut applied = 0usize;
    let mut seen = HashSet::new();
    for &index in target_indices {
        if index == current_index || !seen.insert(index) {
            continue;
        }
        let Some(entry) = app_state.entries.get_mut(index) else {
            continue;
        };
        entry.width_preset = settings.0;
        entry.custom_width = settings.1;
        entry.quality_preset = settings.2;
        entry.custom_quality = settings.3;
        applied += 1;
    }
    applied
}

fn activate_entry_at(
    index: usize,
    sidebar: &SidebarUi,
    content: &ContentUi,
    state: &SharedState,
) {
    let entry = {
        let mut app_state = state.borrow_mut();
        app_state.current_index = Some(index);
        app_state.entries.get(index).cloned()
    };
    let Some(entry) = entry else {
        return;
    };

    sync_resize_ui(&entry, sidebar);
    sync_quality_ui(&entry, sidebar);
    {
        let mut app_state = state.borrow_mut();
        if app_state
            .decoded_source_cache
            .as_ref()
            .is_some_and(|cache| cache.path != entry.path)
        {
            app_state.decoded_source_cache = None;
        }
        if app_state
            .active_render_cache
            .as_ref()
            .is_some_and(|cache| cache.path != entry.path)
        {
            app_state.active_render_cache = None;
        }
    }
    refresh_action_state(sidebar, content, state);
    refresh_selected_preview(sidebar, content, state);
}

fn connect_queue_state(sidebar: &SidebarUi, content: &ContentUi, state: &SharedState) {
    let clear_button = sidebar.clear_button.clone();
    let sidebar_for_clear = sidebar.clone();
    let content_for_clear = content.clone();
    let state_for_clear = state.clone();
    clear_button.connect_clicked(move |_| {
        let mut app_state = state_for_clear.borrow_mut();
        app_state.entries.clear();
        app_state.current_index = None;
        app_state.bulk_selected.clear();
        app_state.decoded_source_cache = None;
        app_state.active_render_cache = None;
        app_state.preview_generation = app_state.preview_generation.wrapping_add(1);
        drop(app_state);
        refresh_queue(&sidebar_for_clear, &content_for_clear, &state_for_clear);
    });

    let sidebar_for_current = sidebar.clone();
    let content_for_current = content.clone();
    let state_for_current = state.clone();
    content.listbox.connect_row_selected(move |_, row| {
        let Some(row) = row else {
            return;
        };
        let index = row.index();
        if index < 0 {
            return;
        }
        activate_entry_at(
            index as usize,
            &sidebar_for_current,
            &content_for_current,
            &state_for_current,
        );
    });

}

fn preview_cache_key(entry: &ImageEntry, options: &ProcessOptions) -> PreviewCacheKey {
    PreviewCacheKey {
        path: entry.path.clone(),
        target_width: options.target_width,
        quality_preset: options.quality_preset,
        custom_quality: options.custom_quality,
    }
}

fn ensure_render_cache_path(app_state: &mut AppState, path: &std::path::Path) {
    if app_state
        .active_render_cache
        .as_ref()
        .is_none_or(|cache| cache.path != path)
    {
        app_state.active_render_cache = Some(ActiveRenderCache {
            path: path.to_path_buf(),
            original_texture: None,
            preview: None,
        });
    }
}

fn cached_preview(
    entry: &ImageEntry,
    options: &ProcessOptions,
    state: &SharedState,
) -> Option<PreviewRenderCache> {
    let key = preview_cache_key(entry, options);
    state
        .borrow()
        .active_render_cache
        .as_ref()
        .filter(|cache| cache.path == entry.path)
        .and_then(|cache| cache.preview.as_ref())
        .filter(|preview| preview.key == key)
        .cloned()
}

fn store_preview_cache(
    entry: &ImageEntry,
    options: &ProcessOptions,
    texture: gtk::gdk::Texture,
    output_width: u32,
    output_height: u32,
    original_bytes: u64,
    encoded_size: u64,
    state: &SharedState,
) {
    let mut app_state = state.borrow_mut();
    ensure_render_cache_path(&mut app_state, &entry.path);
    if let Some(cache) = app_state.active_render_cache.as_mut() {
        cache.preview = Some(PreviewRenderCache {
            key: preview_cache_key(entry, options),
            texture,
            output_width,
            output_height,
            original_bytes,
            encoded_size,
        });
    }
}

fn cached_or_materialized_original(
    entry: &ImageEntry,
    state: &SharedState,
) -> Option<gtk::gdk::Texture> {
    if let Some(texture) = state
        .borrow()
        .active_render_cache
        .as_ref()
        .filter(|cache| cache.path == entry.path)
        .and_then(|cache| cache.original_texture.clone())
    {
        return Some(texture);
    }

    let decoded = state
        .borrow()
        .decoded_source_cache
        .as_ref()
        .filter(|cache| cache.path == entry.path)
        .map(|cache| cache.image.clone())?;
    let started = Instant::now();
    let texture = image_io::texture_from_raw(decoded.as_ref());
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    let mut app_state = state.borrow_mut();
    ensure_render_cache_path(&mut app_state, &entry.path);
    if let Some(cache) = app_state.active_render_cache.as_mut() {
        cache.original_texture = Some(texture.clone());
    }
    eprintln!(
        "[Image Bench performance] stage=render-cache mode=original status=materialized path={} total_ms={elapsed_ms:.2}",
        entry.path.display(),
    );
    Some(texture)
}

fn show_cached_preview_mode(
    entry: &ImageEntry,
    sidebar: &SidebarUi,
    content: &ContentUi,
    state: &SharedState,
) -> bool {
    let display_name = entry
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| tr("Image"));
    content.preview_title.set_text(&display_name);

    match content.preview_modes.active() {
        0 => {
            let Some(original) = cached_or_materialized_original(entry, state) else {
                return false;
            };
            content.preview_display.set_visible_child_name("single");
            content.preview.set_paintable(Some(&original));
            content.preview_meta.set_text(&format!(
                "{} × {}  •  {}",
                entry.width,
                entry.height,
                human_bytes(entry.size_bytes)
            ));
            eprintln!(
                "[Image Bench performance] stage=render-cache mode=original status=hit path={}",
                entry.path.display(),
            );
            true
        }
        1 => {
            let options = process_options_for_entry(sidebar, entry);
            let Some(preview) = cached_preview(entry, &options, state) else {
                return false;
            };
            let Some(original) = cached_or_materialized_original(entry, state) else {
                return false;
            };
            content.preview_display.set_visible_child_name("compare");
            content.compare.set_textures(original, preview.texture.clone());
            content.preview_meta.set_text(&preview_result_text(
                preview.output_width,
                preview.output_height,
                preview.original_bytes,
                preview.encoded_size,
            ));
            eprintln!(
                "[Image Bench performance] stage=render-cache mode=compare status=hit path={}",
                entry.path.display(),
            );
            true
        }
        _ => {
            let options = process_options_for_entry(sidebar, entry);
            let Some(preview) = cached_preview(entry, &options, state) else {
                return false;
            };
            content.preview_display.set_visible_child_name("single");
            content.preview.set_paintable(Some(&preview.texture));
            content.preview_meta.set_text(&preview_result_text(
                preview.output_width,
                preview.output_height,
                preview.original_bytes,
                preview.encoded_size,
            ));
            eprintln!(
                "[Image Bench performance] stage=render-cache mode=preview status=hit path={}",
                entry.path.display(),
            );
            true
        }
    }
}

fn refresh_selected_preview(sidebar: &SidebarUi, content: &ContentUi, state: &SharedState) {
    let entry = {
        let app_state = state.borrow();
        app_state
            .current_index
            .and_then(|index| app_state.entries.get(index).cloned())
    };
    if let Some(entry) = entry {
        if show_cached_preview_mode(&entry, sidebar, content, state) {
            return;
        }
        request_preview(entry, sidebar.clone(), content.clone(), state.clone());
    }
}

fn request_preview(entry: ImageEntry, sidebar: SidebarUi, content: ContentUi, state: SharedState) {
    let generation = {
        let mut app_state = state.borrow_mut();
        app_state.preview_generation = app_state.preview_generation.wrapping_add(1);
        let generation = app_state.preview_generation;

        if app_state.preview_running {
            app_state.preview_pending = true;
            eprintln!(
                "[Image Bench performance] stage=preview-job status=pending generation={generation}"
            );
            return;
        }

        app_state.preview_running = true;
        generation
    };

    if content.preview_spinner.is_visible() {
        content.preview_spinner.start();
    } else {
        let spinner = content.preview_spinner.clone();
        let spinner_state = state.clone();
        glib::timeout_add_local(Duration::from_millis(150), move || {
            let should_show = {
                let app_state = spinner_state.borrow();
                app_state.preview_running && app_state.preview_generation == generation
            };
            if should_show {
                spinner.set_visible(true);
                spinner.start();
            }
            glib::ControlFlow::Break
        });
    }

    show_preview(entry, sidebar, content, state, generation);
}

struct PreviewJobGuard {
    sidebar: SidebarUi,
    content: ContentUi,
    state: SharedState,
}

impl PreviewJobGuard {
    fn new(sidebar: SidebarUi, content: ContentUi, state: SharedState) -> Self {
        Self { sidebar, content, state }
    }
}

impl Drop for PreviewJobGuard {
    fn drop(&mut self) {
        let should_refresh = {
            let mut app_state = self.state.borrow_mut();
            app_state.preview_running = false;
            if app_state.preview_pending {
                app_state.preview_pending = false;
                true
            } else {
                false
            }
        };

        if should_refresh {
            eprintln!("[Image Bench performance] stage=preview-job status=restart-latest");
            refresh_selected_preview(&self.sidebar, &self.content, &self.state);
        } else {
            self.content.preview_spinner.stop();
            self.content.preview_spinner.set_visible(false);
        }
    }
}

async fn decoded_source_for_preview(
    entry: &ImageEntry,
    state: &SharedState,
    generation: u64,
) -> Result<Option<Arc<image_io::RawImage>>, String> {
    if let Some(image) = state
        .borrow()
        .decoded_source_cache
        .as_ref()
        .filter(|cache| cache.path == entry.path)
        .map(|cache| cache.image.clone())
    {
        let still_current = {
            let app_state = state.borrow();
            app_state.preview_generation == generation
                && app_state
                    .entries
                    .iter()
                    .any(|candidate| candidate.path == entry.path)
        };
        if !still_current {
            return Ok(None);
        }

        eprintln!(
            "[Image Bench performance] stage=source-cache path={} status=hit",
            entry.path.display()
        );
        return Ok(Some(image));
    }

    let decode_started = Instant::now();
    let decoded = Arc::new(image_io::decode(&entry.path, None).await?);
    let decode_elapsed = decode_started.elapsed();
    let still_current = {
        let app_state = state.borrow();
        app_state.preview_generation == generation
            && app_state
                .entries
                .iter()
                .any(|candidate| candidate.path == entry.path)
    };
    if !still_current {
        return Ok(None);
    }

    state.borrow_mut().decoded_source_cache = Some(DecodedSourceCache {
        path: entry.path.clone(),
        image: decoded.clone(),
    });
    eprintln!(
        "[Image Bench performance] stage=source-cache path={} status=miss decode_ms={:.2}",
        entry.path.display(),
        decode_elapsed.as_secs_f64() * 1000.0,
    );
    Ok(Some(decoded))
}

fn show_preview(
    entry: ImageEntry,
    sidebar: SidebarUi,
    content: ContentUi,
    state: SharedState,
    generation: u64,
) {

    content.preview.set_paintable(None::<&gtk::gdk::Texture>);
    content.compare.clear();
    let display_name = entry
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| tr("Image"));
    content.preview_title.set_text(&display_name);

    let preview_mode = content.preview_modes.active();
    if preview_mode == 0 {
        content.preview_display.set_visible_child_name("single");
        content.preview_meta.set_text(&format!(
            "{} × {}  •  {}",
            entry.width,
            entry.height,
            human_bytes(entry.size_bytes)
        ));

        let job_guard = PreviewJobGuard::new(sidebar.clone(), content.clone(), state.clone());
        glib::MainContext::default().spawn_local(async move {
            let _job_guard = job_guard;
            let preview_started = Instant::now();
            let decoded = match decoded_source_for_preview(&entry, &state, generation).await {
                Ok(Some(decoded)) => decoded,
                Ok(None) => return,
                Err(error) => {
                    content.preview.set_paintable(None::<&gtk::gdk::Texture>);
                    eprintln!(
                        "[Image Bench] Could not decode original preview source for {}: {error}",
                        entry.path.display()
                    );
                    return;
                }
            };

            let still_current = {
                let app_state = state.borrow();
                app_state.preview_generation == generation
                    && app_state.entries.iter().any(|candidate| candidate.path == entry.path)
            };
            if !still_current {
                return;
            }

            let texture = cached_or_materialized_original(&entry, &state)
                .unwrap_or_else(|| image_io::texture_from_raw(decoded.as_ref()));
            content.preview.set_paintable(Some(&texture));
            eprintln!(
                "[Image Bench performance] stage=original-preview-total path={} total_ms={:.2}",
                entry.path.display(),
                preview_started.elapsed().as_secs_f64() * 1000.0,
            );
        });
        return;
    }

    if preview_mode == 1 {
        content.preview_display.set_visible_child_name("compare");
        content.preview_meta.set_text(&tr("Generating comparison…"));
        let options = process_options_for_entry(&sidebar, &entry);
        let job_guard = PreviewJobGuard::new(sidebar.clone(), content.clone(), state.clone());

        glib::MainContext::default().spawn_local(async move {
            let _job_guard = job_guard;
            let compare_started = Instant::now();
            let decoded = match decoded_source_for_preview(&entry, &state, generation).await {
                Ok(Some(decoded)) => decoded,
                Ok(None) => return,
                Err(error) => {
                    if state.borrow().preview_generation == generation {
                        content.compare.clear();
                        content.preview_meta.set_text(&tr("Compare unavailable"));
                    }
                    eprintln!(
                        "[Image Bench] Could not decode cached comparison source for {}: {error}",
                        entry.path.display()
                    );
                    return;
                }
            };

            let Some(original_texture) = cached_or_materialized_original(&entry, &state) else {
                if state.borrow().preview_generation == generation {
                    content.compare.clear();
                    content.preview_meta.set_text(&tr("Compare unavailable"));
                }
                eprintln!(
                    "[Image Bench] Could not materialize original comparison texture for {}",
                    entry.path.display()
                );
                return;
            };

            let rendered = match processor::render_preview_with_decoded(
                &entry.path,
                Some(decoded),
                &options,
                None,
            )
            .await
            {
                Ok(rendered) => rendered,
                Err(error) => {
                    if state.borrow().preview_generation == generation {
                        content.compare.clear();
                        content.preview_meta.set_text(&tr("Compare unavailable"));
                    }
                    eprintln!(
                        "[Image Bench] Could not generate comparison preview for {}: {error}",
                        entry.path.display()
                    );
                    return;
                }
            };

            let encoded_size = rendered.encoded_data.len() as u64;
            let preview_decode_started = Instant::now();
            let preview_texture = match image_io::decode_texture_vec(rendered.encoded_data, None).await {
                Ok(texture) => texture,
                Err(error) => {
                    if state.borrow().preview_generation == generation {
                        content.compare.clear();
                        content.preview_meta.set_text(&tr("Compare unavailable"));
                    }
                    eprintln!(
                        "[Image Bench] Could not decode generated comparison preview for {}: {error}",
                        entry.path.display()
                    );
                    return;
                }
            };
            eprintln!(
                "[Image Bench performance] stage=preview-decode mode=compare path={} decode_ms={:.2}",
                entry.path.display(),
                preview_decode_started.elapsed().as_secs_f64() * 1000.0,
            );

            let still_current = {
                let app_state = state.borrow();
                app_state.preview_generation == generation
                    && app_state.entries.iter().any(|candidate| candidate.path == entry.path)
            };
            if !still_current {
                return;
            }

            store_preview_cache(
                &entry,
                &options,
                preview_texture.clone(),
                rendered.output_width,
                rendered.output_height,
                rendered.original_bytes,
                encoded_size,
                &state,
            );
            content.compare.set_textures(original_texture, preview_texture);
            content.preview_meta.set_text(&preview_result_text(
                rendered.output_width,
                rendered.output_height,
                rendered.original_bytes,
                encoded_size,
            ));
            eprintln!(
                "[Image Bench performance] stage=compare-total path={} total_ms={:.2}",
                entry.path.display(),
                compare_started.elapsed().as_secs_f64() * 1000.0,
            );
        });
        return;
    }

    content.preview_display.set_visible_child_name("single");
    content.preview_meta.set_text(&tr("Generating preview…"));
    let options = process_options_for_entry(&sidebar, &entry);
    let job_guard = PreviewJobGuard::new(sidebar.clone(), content.clone(), state.clone());
    glib::MainContext::default().spawn_local(async move {
        let _job_guard = job_guard;
        let preview_started = Instant::now();
        let decoded = match decoded_source_for_preview(&entry, &state, generation).await {
            Ok(Some(decoded)) => decoded,
            Ok(None) => return,
            Err(error) => {
                content.preview.set_paintable(None::<&gtk::gdk::Texture>);
                content.preview_meta.set_text(&tr("Preview unavailable"));
                eprintln!(
                    "[Image Bench] Could not decode cached preview source for {}: {error}",
                    entry.path.display()
                );
                return;
            }
        };
        let rendered = processor::render_preview_with_decoded(
            &entry.path,
            Some(decoded),
            &options,
            None,
        )
        .await;
        let still_current = {
            let app_state = state.borrow();
            app_state.preview_generation == generation
                && app_state.entries.iter().any(|candidate| candidate.path == entry.path)
        };
        if !still_current {
            return;
        }

        match rendered {
            Ok(rendered) => {
                let encoded_size = rendered.encoded_data.len() as u64;
                let preview_decode_started = Instant::now();
                match image_io::decode_texture_vec(rendered.encoded_data, None).await {
                    Ok(texture) => {
                        eprintln!(
                            "[Image Bench performance] stage=preview-decode mode=preview path={} decode_ms={:.2}",
                            entry.path.display(),
                            preview_decode_started.elapsed().as_secs_f64() * 1000.0,
                        );
                        let still_current = {
                            let app_state = state.borrow();
                            app_state.preview_generation == generation
                                && app_state.entries.iter().any(|candidate| candidate.path == entry.path)
                        };
                        if !still_current {
                            return;
                        }
                        store_preview_cache(
                            &entry,
                            &options,
                            texture.clone(),
                            rendered.output_width,
                            rendered.output_height,
                            rendered.original_bytes,
                            encoded_size,
                            &state,
                        );
                        content.preview.set_paintable(Some(&texture));
                        content.preview_meta.set_text(&preview_result_text(
                            rendered.output_width,
                            rendered.output_height,
                            rendered.original_bytes,
                            encoded_size,
                        ));
                    }
                    Err(error) => {
                        content.preview.set_paintable(None::<&gtk::gdk::Texture>);
                        content.preview_meta.set_text(&tr("Preview unavailable"));
                        eprintln!(
                            "[Image Bench] Could not decode generated preview for {}: {error}",
                            entry.path.display()
                        );
                    }
                }
            }
            Err(error) => {
                content.preview.set_paintable(None::<&gtk::gdk::Texture>);
                content.preview_meta.set_text(&tr("Preview unavailable"));
                eprintln!(
                    "[Image Bench] Could not generate preview for {}: {error}",
                    entry.path.display()
                );
            }
        }
        eprintln!(
            "[Image Bench performance] stage=preview-total path={} total_ms={:.2}",
            entry.path.display(),
            preview_started.elapsed().as_secs_f64() * 1000.0,
        );
    });
}

fn preview_result_text(width: u32, height: u32, before: u64, after: u64) -> String {
    let size = human_bytes(after);
    if before == 0 || before == after {
        return format!("{width} × {height}  •  {size}");
    }

    let percent = if after < before {
        (before - after) as f64 / before as f64 * 100.0
    } else {
        (after - before) as f64 / before as f64 * 100.0
    };
    let args = [
        ("width", width.to_string()),
        ("height", height.to_string()),
        ("size", size),
        ("percent", format!("{percent:.1}")),
    ];
    if after < before {
        tr_args(
            "{width} × {height}  •  {size}  •  {percent}% smaller",
            &args,
        )
    } else {
        tr_args(
            "{width} × {height}  •  {size}  •  {percent}% larger",
            &args,
        )
    }
}

fn refresh_action_state(sidebar: &SidebarUi, content: &ContentUi, state: &SharedState) {
    let app_state = state.borrow();
    let count = app_state.entries.len();
    let busy = app_state.importing || app_state.processing;
    let custom_quality_pending =
        sidebar.quality_row.selected() == 3 && sidebar.custom_quality_dirty.get();
    let ready = count > 0
        && app_state.output_dir.is_some()
        && !busy
        && !custom_quality_pending;

    let optimize_label = if count == 1 {
        tr("Optimize Image")
    } else {
        tr("Optimize Images")
    };
    sidebar.optimize_button.set_label(&optimize_label);
    sidebar.optimize_button.set_sensitive(ready);
    sidebar.width_row.set_sensitive(!busy);
    sidebar.custom_width_row.set_sensitive(!busy);
    sidebar.quality_row.set_sensitive(!busy);
    sidebar.custom_quality_row.set_sensitive(!busy);
    sidebar.apply_quality_button.set_sensitive(
        !busy && sidebar.custom_quality_dirty.get() && sidebar.quality_row.selected() == 3,
    );

    let selected_target_count = app_state
        .entries
        .iter()
        .enumerate()
        .filter(|(index, entry)| {
            app_state.current_index != Some(*index) && app_state.bulk_selected.contains(&entry.path)
        })
        .count();
    sidebar
        .apply_settings_menu
        .set_sensitive(count > 1 && !busy && !custom_quality_pending);
    sidebar
        .apply_selected_settings_button
        .set_sensitive(selected_target_count > 0 && !busy && !custom_quality_pending);
    sidebar
        .apply_all_settings_button
        .set_sensitive(count > 1 && !busy && !custom_quality_pending);

    sidebar.add_images_button.set_sensitive(!busy);
    sidebar.add_folder_button.set_sensitive(!busy);
    sidebar.clear_button.set_sensitive(count > 0 && !busy);
    sidebar.choose_output_button.set_sensitive(!busy);
    sidebar.filename_suffix_check.set_sensitive(!busy);
    sidebar.filename_suffix_entry.set_sensitive(!busy);
    sidebar.quality_suffix_check.set_sensitive(!busy);
    content.listbox.set_sensitive(!busy);
    content.queue_button.set_sensitive(count > 0);
    content.preview_modes.set_sensitive(count > 0 && !busy);
    refresh_zoom_state(content);
}

fn refresh_queue(sidebar: &SidebarUi, content: &ContentUi, state: &SharedState) {
    while let Some(row) = content.listbox.row_at_index(0) {
        content.listbox.remove(&row);
    }
    content.queue_settings_labels.borrow_mut().clear();

    let (entries, importing, bulk_selected) = {
        let app_state = state.borrow();
        (
            app_state.entries.clone(),
            app_state.importing,
            app_state.bulk_selected.clone(),
        )
    };
    let count = entries.len();
    let count_text = if count == 0 {
        tr("0 images")
    } else {
        trn_args(
            "{count} image",
            "{count} images",
            count as u32,
            &[("count", count.to_string())],
        )
    };

    content.queue_count_label.set_text(&count_text);
    let queue_subtitle = if importing {
        tr("Inspecting images…")
    } else if count == 0 {
        tr("No images loaded")
    } else {
        count_text.clone()
    };
    sidebar.queue_row.set_subtitle(&queue_subtitle);
    refresh_action_state(sidebar, content, state);
    content
        .stack
        .set_visible_child_name(if count == 0 { "empty" } else { "queue" });
    if count == 0 {
        content.queue_split.set_show_sidebar(false);
    } else if !content.queue_button.is_visible() {
        content.queue_split.set_show_sidebar(true);
    }

    if count == 0 {
        content.preview.set_paintable(None::<&gtk::gdk::Texture>);
        content.compare.clear();
        content.zoom_row.set_selected(0);
        content.preview_display.set_visible_child_name("single");
        content.preview_title.set_text("");
        content.preview_meta.set_text("");
        return;
    }

    for entry in entries {
        let row = gtk::ListBoxRow::new();
        let row_box = gtk::Box::new(Orientation::Horizontal, 8);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(8);
        row_box.set_margin_start(8);
        row_box.set_margin_end(8);
        row.set_child(Some(&row_box));

        let bulk_check = gtk::CheckButton::new();
        bulk_check.set_active(bulk_selected.contains(&entry.path));
        bulk_check.set_valign(Align::Center);
        bulk_check.set_tooltip_text(Some(&tr("Select for bulk actions")));
        row_box.append(&bulk_check);

        let bulk_path = entry.path.clone();
        let sidebar_for_bulk = sidebar.clone();
        let content_for_bulk = content.clone();
        let state_for_bulk = state.clone();
        bulk_check.connect_toggled(move |check| {
            {
                let mut app_state = state_for_bulk.borrow_mut();
                if check.is_active() {
                    app_state.bulk_selected.insert(bulk_path.clone());
                } else {
                    app_state.bulk_selected.remove(&bulk_path);
                }
            }
            refresh_action_state(&sidebar_for_bulk, &content_for_bulk, &state_for_bulk);
        });

        let thumbnail_cell = gtk::Box::new(Orientation::Horizontal, 0);
        thumbnail_cell.set_size_request(72, 56);
        thumbnail_cell.set_hexpand(false);
        thumbnail_cell.set_vexpand(false);
        thumbnail_cell.set_halign(Align::Center);
        thumbnail_cell.set_valign(Align::Center);

        let thumbnail = gtk::Picture::new();
        thumbnail.set_content_fit(gtk::ContentFit::Contain);
        thumbnail.set_can_shrink(true);
        thumbnail.set_size_request(72, 56);
        thumbnail.set_hexpand(false);
        thumbnail.set_vexpand(false);
        thumbnail.set_halign(Align::Center);
        thumbnail.set_valign(Align::Center);
        thumbnail_cell.append(&thumbnail);
        row_box.append(&thumbnail_cell);

        let thumbnail_path = entry.path.clone();
        let thumbnail_state = state.clone();
        let thumbnail_picture = thumbnail.clone();
        glib::MainContext::default().spawn_local(async move {
            match image_io::decode_texture(&thumbnail_path, None).await {
                Ok(texture) => {
                    let still_present = thumbnail_state
                        .borrow()
                        .entries
                        .iter()
                        .any(|candidate| candidate.path == thumbnail_path);
                    if still_present {
                        thumbnail_picture.set_paintable(Some(&texture));
                    }
                }
                Err(error) => eprintln!(
                    "[Image Bench] Could not load queue thumbnail for {}: {error}",
                    thumbnail_path.display()
                ),
            }
        });

        let labels = gtk::Box::new(Orientation::Vertical, 3);
        labels.set_hexpand(true);
        labels.set_valign(Align::Center);
        let title = gtk::Label::builder()
            .label(entry.path.file_name().and_then(|name| name.to_str()).unwrap_or("Image"))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("heading");
        let meta = gtk::Label::builder()
            .label(format!(
                "{} × {}  •  {}",
                entry.width,
                entry.height,
                human_bytes(entry.size_bytes)
            ))
            .xalign(0.0)
            .build();
        meta.add_css_class("dim-label");
        let settings = gtk::Label::builder()
            .label(entry_settings_text(&entry))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        settings.add_css_class("dim-label");
        labels.append(&title);
        labels.append(&meta);
        labels.append(&settings);
        content.queue_settings_labels.borrow_mut().push(settings);
        row_box.append(&labels);

        let remove = gtk::Button::builder()
            .icon_name(resolved_icon_name(
                "window-close-symbolic",
                "list-remove-symbolic",
            ))
            .build();
        remove.add_css_class("flat");
        remove.set_width_request(36);
        remove.set_height_request(36);
        remove.set_hexpand(false);
        remove.set_vexpand(false);
        remove.set_halign(Align::Center);
        remove.set_valign(Align::Center);
        remove.set_tooltip_text(Some(&tr("Remove from queue")));

        let remove_hover = gtk::EventControllerMotion::new();
        let remove_for_enter = remove.clone();
        remove_hover.connect_enter(move |_, _, _| {
            remove_for_enter.add_css_class("destructive-action");
        });
        let remove_for_leave = remove.clone();
        remove_hover.connect_leave(move |_| {
            remove_for_leave.remove_css_class("destructive-action");
        });
        remove.add_controller(remove_hover);

        row_box.append(&remove);

        let path = entry.path.clone();
        let sidebar_for_remove = sidebar.clone();
        let content_for_remove = content.clone();
        let state_for_remove = state.clone();
        remove.connect_clicked(move |_| {
            {
                let mut app_state = state_for_remove.borrow_mut();
                app_state.bulk_selected.remove(&path);
                app_state.entries.retain(|entry| entry.path != path);
                app_state.current_index = if app_state.entries.is_empty() { None } else { Some(0) };
                app_state.decoded_source_cache = None;
                app_state.active_render_cache = None;
                app_state.preview_generation = app_state.preview_generation.wrapping_add(1);
            }
            refresh_queue(
                &sidebar_for_remove,
                &content_for_remove,
                &state_for_remove,
            );
            if let Some(first_row) = content_for_remove.listbox.row_at_index(0) {
                content_for_remove.listbox.select_row(Some(&first_row));
            }
        });

        content.listbox.append(&row);
    }
}

fn resolved_icon_name(primary: &'static str, fallback: &'static str) -> &'static str {
    let Some(display) = gtk::gdk::Display::default() else {
        eprintln!(
            "[Image Bench icons] display unavailable while resolving {primary}; fallback={fallback}"
        );
        return fallback;
    };
    let icon_theme = gtk::IconTheme::for_display(&display);
    if icon_theme.has_icon(primary) {
        primary
    } else {
        eprintln!("[Image Bench icons] missing={primary} fallback={fallback}");
        fallback
    }
}

fn section_info_indicator(tooltip: &str) -> gtk::Widget {
    let icon_name = resolved_icon_name(
        "dialog-information-symbolic",
        "help-contents-symbolic",
    );

    // A generous hover target with icon-button-like feedback, but deliberately
    // no GtkButton semantics, click handler, pressed state, or keyboard action.
    let target = gtk::Grid::new();
    target.set_size_request(36, 36);
    target.set_hexpand(false);
    target.set_vexpand(false);
    target.set_halign(Align::Center);
    target.set_valign(Align::Center);
    target.set_margin_end(12);
    target.set_can_target(true);
    target.set_tooltip_text(Some(tooltip));
    target.add_css_class("image-bench-info-target");

    let image = gtk::Image::from_icon_name(icon_name);
    image.set_pixel_size(16);
    image.set_halign(Align::Center);
    image.set_valign(Align::Center);
    image.set_can_target(false);
    image.add_css_class("image-bench-info-indicator");
    target.attach(&image, 0, 0, 1, 1);

    let motion = gtk::EventControllerMotion::new();
    let hover_target = target.downgrade();
    motion.connect_enter(move |_, _, _| {
        if let Some(target) = hover_target.upgrade() {
            target.add_css_class("image-bench-info-target-hover");
        }
    });
    let leave_target = target.downgrade();
    motion.connect_leave(move |_| {
        if let Some(target) = leave_target.upgrade() {
            target.remove_css_class("image-bench-info-target-hover");
        }
    });
    target.add_controller(motion);

    target.upcast()
}

fn build_sidebar() -> SidebarUi {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let title = adw::WindowTitle::new("Image Bench", &tr("Local image optimization"));
    header.set_title_widget(Some(&title));

    let app_menu = gio::Menu::new();
    app_menu.append(Some(&tr("About Image Bench")), Some("app.about"));
    app_menu.append(Some(&tr("Quit")), Some("app.quit"));
    let app_menu_button = gtk::MenuButton::builder()
        .menu_model(&app_menu)
        .icon_name("open-menu-symbolic")
        .tooltip_text(tr("Main menu"))
        .build();
    header.pack_end(&app_menu_button);
    toolbar.add_top_bar(&header);

    let sidebar_box = gtk::Box::new(Orientation::Vertical, 0);
    toolbar.set_content(Some(&sidebar_box));

    let page = adw::PreferencesPage::new();
    page.set_vexpand(true);
    sidebar_box.append(&page);

    let images_group = adw::PreferencesGroup::builder().title(tr("1. Images")).build();
    page.add(&images_group);

    let queue_row = adw::ActionRow::builder()
        .title(tr("Queue"))
        .subtitle(tr("No images loaded"))
        .build();
    let clear_button = gtk::Button::builder()
        .icon_name(resolved_icon_name("edit-clear-symbolic", "edit-clear-all-symbolic"))
        .valign(Align::Center)
        .tooltip_text(tr("Clear queue"))
        .build();
    let clear_hover = gtk::EventControllerMotion::new();
    let clear_for_enter = clear_button.clone();
    clear_hover.connect_enter(move |_, _, _| {
        clear_for_enter.add_css_class("destructive-action");
    });
    let clear_for_leave = clear_button.clone();
    clear_hover.connect_leave(move |_| {
        clear_for_leave.remove_css_class("destructive-action");
    });
    clear_button.add_controller(clear_hover);

    queue_row.add_suffix(&clear_button);
    images_group.add(&queue_row);

    let add_box = gtk::Box::new(Orientation::Horizontal, 6);
    add_box.set_homogeneous(true);
    add_box.set_margin_top(8);
    let add_images_button = gtk::Button::with_label(&tr("Add Images"));
    add_images_button.add_css_class("suggested-action");
    let add_folder_button = gtk::Button::with_label(&tr("Add Folder"));
    add_box.append(&add_images_button);
    add_box.append(&add_folder_button);
    images_group.add(&add_box);

    let resize_group = adw::PreferencesGroup::builder()
        .title(tr("2. Resize"))
        .build();
    let resize_info = section_info_indicator(&tr(
        "Aspect ratio is always preserved. Images are never upscaled.",
    ));
    resize_group.set_header_suffix(Some(&resize_info));
    page.add(&resize_group);

    let width_labels = [
        tr("1920 px — Background / Hero"),
        tr("1280 px — Content"),
        tr("800 px — Small"),
        tr("Custom width"),
    ];
    let width_label_refs: Vec<&str> = width_labels.iter().map(String::as_str).collect();
    let widths = gtk::StringList::new(&width_label_refs);
    let width_row = adw::ComboRow::builder()
        .title(tr("Target width"))
        .model(&widths)
        .selected(1)
        .build();
    resize_group.add(&width_row);

    let custom_width_row = adw::SpinRow::with_range(64.0, 16384.0, 1.0);
    custom_width_row.set_title(&tr("Custom width"));
    custom_width_row.set_value(1280.0);
    custom_width_row.set_visible(false);
    resize_group.add(&custom_width_row);

    let quality_group = adw::PreferencesGroup::builder().title(tr("3. Quality")).build();
    let quality_info = section_info_indicator(&tr("JPEG quality; PNG remains lossless"));
    quality_group.set_header_suffix(Some(&quality_info));
    page.add(&quality_group);
    let quality_labels = [
        tr("High — 92%"),
        tr("Balanced — 85%"),
        tr("Compact — 75%"),
        tr("Custom"),
    ];
    let quality_label_refs: Vec<&str> = quality_labels.iter().map(String::as_str).collect();
    let quality_levels = gtk::StringList::new(&quality_label_refs);
    let quality_row = adw::ComboRow::builder()
        .title(tr("Quality level"))
        .model(&quality_levels)
        .selected(1)
        .build();
    quality_group.add(&quality_row);

    let applied_custom_quality = Rc::new(Cell::new(85u8));
    let custom_quality_dirty = Rc::new(Cell::new(false));
    let syncing_quality_ui = Rc::new(Cell::new(false));
    let syncing_resize_ui = Rc::new(Cell::new(false));
    let custom_quality_row = adw::SpinRow::with_range(1.0, 100.0, 1.0);
    custom_quality_row.set_title(&tr("Quality (%)"));
    custom_quality_row.set_subtitle(&tr("JPEG quality; PNG remains lossless"));
    custom_quality_row.set_value(85.0);
    custom_quality_row.set_visible(false);
    quality_group.add(&custom_quality_row);

    let apply_quality_row = adw::PreferencesRow::new();
    apply_quality_row.set_activatable(false);
    apply_quality_row.set_selectable(false);
    apply_quality_row.set_visible(false);
    let apply_quality_button = gtk::Button::with_label(&tr("Apply"));
    apply_quality_button.add_css_class("suggested-action");
    apply_quality_button.set_hexpand(true);
    apply_quality_button.set_margin_top(6);
    apply_quality_button.set_margin_bottom(6);
    apply_quality_button.set_margin_start(12);
    apply_quality_button.set_margin_end(12);
    apply_quality_button.set_sensitive(false);
    apply_quality_row.set_child(Some(&apply_quality_button));
    quality_group.add(&apply_quality_row);

    let apply_settings_row = adw::ActionRow::builder()
        .title(tr("Apply current settings"))
        .subtitle(tr("Copy Resize and Quality to other images"))
        .build();
    let apply_settings_menu = gtk::MenuButton::builder()
        .label(tr("Apply…"))
        .valign(Align::Center)
        .build();
    let apply_settings_popover = gtk::Popover::new();
    let apply_settings_box = gtk::Box::new(Orientation::Vertical, 4);
    apply_settings_box.set_margin_top(6);
    apply_settings_box.set_margin_bottom(6);
    apply_settings_box.set_margin_start(6);
    apply_settings_box.set_margin_end(6);
    let apply_selected_settings_button = gtk::Button::with_label(&tr("Apply to Selected Images"));
    apply_selected_settings_button.add_css_class("flat");
    apply_selected_settings_button.set_sensitive(false);
    let apply_all_settings_button = gtk::Button::with_label(&tr("Apply to All Images"));
    apply_all_settings_button.add_css_class("flat");
    apply_all_settings_button.set_sensitive(false);
    apply_settings_box.append(&apply_selected_settings_button);
    apply_settings_box.append(&apply_all_settings_button);
    apply_settings_popover.set_child(Some(&apply_settings_box));
    apply_settings_menu.set_popover(Some(&apply_settings_popover));
    apply_settings_menu.set_sensitive(false);
    apply_settings_row.add_suffix(&apply_settings_menu);
    quality_group.add(&apply_settings_row);

    let output_group = adw::PreferencesGroup::builder().title(tr("4. Output")).build();
    let output_info = section_info_indicator(&tr("Image Bench never overwrites source files."));
    output_group.set_header_suffix(Some(&output_info));
    page.add(&output_group);

    let output_row = adw::ActionRow::builder()
        .title(tr("Output folder"))
        .subtitle(tr("Choose where optimized images are saved"))
        .build();
    let choose_output_button = gtk::Button::builder()
        .label(tr("Choose"))
        .valign(Align::Center)
        .build();
    output_row.add_suffix(&choose_output_button);
    output_group.add(&output_row);

    let advanced_output = adw::ExpanderRow::builder()
        .title(tr("Advanced output options"))
        .subtitle(tr("Filename and quality suffixes"))
        .expanded(false)
        .build();
    output_group.add(&advanced_output);

    let filename_suffix_row = adw::ActionRow::builder()
        .title(tr("Add filename suffix"))
        .subtitle(tr("Adds a custom suffix before the file extension"))
        .build();
    let filename_suffix_check = gtk::CheckButton::builder()
        .valign(Align::Center)
        .active(true)
        .build();
    filename_suffix_row.add_suffix(&filename_suffix_check);
    advanced_output.add_row(&filename_suffix_row);

    let filename_suffix_entry = adw::EntryRow::builder().title(tr("Filename suffix")).build();
    filename_suffix_entry.set_text("-optimized");
    advanced_output.add_row(&filename_suffix_entry);

    let quality_suffix_row = adw::ActionRow::builder()
        .title(tr("Add quality suffix"))
        .subtitle(tr("Adds -85 for the current quality setting"))
        .build();
    let quality_suffix_check = gtk::CheckButton::builder()
        .valign(Align::Center)
        .active(false)
        .build();
    quality_suffix_row.add_suffix(&quality_suffix_check);
    advanced_output.add_row(&quality_suffix_row);

    let optimize_button = gtk::Button::with_label(&tr("Optimize Images"));
    optimize_button.add_css_class("suggested-action");
    optimize_button.add_css_class("pill");
    optimize_button.set_margin_top(12);
    optimize_button.set_margin_bottom(18);
    optimize_button.set_margin_start(12);
    optimize_button.set_margin_end(12);
    optimize_button.set_sensitive(false);
    sidebar_box.append(&optimize_button);

    SidebarUi {
        toolbar,
        add_images_button,
        add_folder_button,
        clear_button,
        queue_row,
        width_row,
        custom_width_row,
        quality_row,
        custom_quality_row,
        apply_quality_row,
        apply_quality_button,
        applied_custom_quality,
        custom_quality_dirty,
        syncing_quality_ui,
        syncing_resize_ui,
        apply_settings_menu,
        apply_selected_settings_button,
        apply_all_settings_button,
        output_row,
        choose_output_button,
        filename_suffix_check,
        filename_suffix_entry,
        quality_suffix_row,
        quality_suffix_check,
        optimize_button,
    }
}


fn connect_output_folder(
    window: &adw::ApplicationWindow,
    sidebar: &SidebarUi,
    content: &ContentUi,
    state: &SharedState,
) {
    let choose_button = sidebar.choose_output_button.clone();
    let window_for_output = window.clone();
    let output_row = sidebar.output_row.clone();
    let sidebar_for_output = sidebar.clone();
    let content_for_output = content.clone();
    let state_for_output = state.clone();

    choose_button.connect_clicked(move |_| {
        let dialog = gtk::FileDialog::builder()
            .title(tr("Choose Output Folder"))
            .modal(true)
            .build();
        let window = window_for_output.clone();
        let output_row = output_row.clone();
        let sidebar = sidebar_for_output.clone();
        let content = content_for_output.clone();
        let state = state_for_output.clone();

        glib::MainContext::default().spawn_local(async move {
            let Ok(folder) = dialog.select_folder_future(Some(&window)).await else {
                return;
            };
            let Some(path) = folder.path() else {
                return;
            };

            output_row.set_subtitle(&path.to_string_lossy());
            state.borrow_mut().output_dir = Some(path);
            refresh_action_state(&sidebar, &content, &state);
        });
    });
}

fn process_options(sidebar: &SidebarUi) -> ProcessOptions {
    let (quality_preset, custom_quality) = quality_settings(
        &sidebar.quality_row,
        sidebar.applied_custom_quality.get(),
    );
    ProcessOptions {
        target_width: match sidebar.width_row.selected() {
            0 => 1920,
            1 => 1280,
            2 => 800,
            _ => sidebar.custom_width_row.value() as u32,
        },
        quality_preset,
        custom_quality,
        add_filename_suffix: sidebar.filename_suffix_check.is_active(),
        filename_suffix: sidebar.filename_suffix_entry.text().trim().to_string(),
        add_quality_suffix: sidebar.quality_suffix_check.is_active(),
    }
}

fn process_options_for_entry(sidebar: &SidebarUi, entry: &ImageEntry) -> ProcessOptions {
    let mut options = process_options(sidebar);
    options.target_width = entry_target_width(entry);
    options.quality_preset = entry.quality_preset;
    options.custom_quality = entry.custom_quality;
    options
}

fn connect_optimize(
    toast_overlay: &adw::ToastOverlay,
    sidebar: &SidebarUi,
    content: &ContentUi,
    state: &SharedState,
) {
    let button = sidebar.optimize_button.clone();
    let toast_overlay = toast_overlay.clone();
    let sidebar_for_optimize = sidebar.clone();
    let content_for_optimize = content.clone();
    let state_for_optimize = state.clone();

    button.connect_clicked(move |_| {
        let (entries, output_dir) = {
            let mut app_state = state_for_optimize.borrow_mut();
            if app_state.processing
                || app_state.importing
                || app_state.entries.is_empty()
                || app_state.output_dir.is_none()
            {
                return;
            }
            app_state.processing = true;
            (
                app_state.entries.clone(),
                app_state.output_dir.clone().expect("checked output directory"),
            )
        };

        refresh_action_state(
            &sidebar_for_optimize,
            &content_for_optimize,
            &state_for_optimize,
        );

        let count = entries.len();
        content_for_optimize.progress_box.set_visible(true);
        content_for_optimize.progress.set_fraction(0.0);
        content_for_optimize.progress_label.set_text(&trn_args(
            "Preparing {count} image…",
            "Preparing {count} images…",
            count as u32,
            &[("count", count.to_string())],
        ));

        let base_options = process_options(&sidebar_for_optimize);

        let sidebar = sidebar_for_optimize.clone();
        let content = content_for_optimize.clone();
        let state = state_for_optimize.clone();
        let toast_overlay = toast_overlay.clone();
        glib::MainContext::default().spawn_local(async move {
            let total = entries.len();
            let mut results: Vec<ProcessResult> = Vec::new();
            let mut errors: Vec<(PathBuf, String)> = Vec::new();

            for (index, entry) in entries.into_iter().enumerate() {
                content.progress.set_fraction(index as f64 / total as f64);
                let name = entry
                    .path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| tr("Image"));
                content.progress_label.set_text(&tr_args(
                    "Optimizing {name}  •  {current} of {total}",
                    &[
                        ("name", name.to_string()),
                        ("current", (index + 1).to_string()),
                        ("total", total.to_string()),
                    ],
                ));

                let mut options = base_options.clone();
                options.target_width = entry_target_width(&entry);
                options.quality_preset = entry.quality_preset;
                options.custom_quality = entry.custom_quality;

                match processor::process(&entry.path, &output_dir, &options, None).await {
                    Ok(result) => results.push(result),
                    Err(error) => errors.push((entry.path, error)),
                }
            }

            state.borrow_mut().processing = false;
            content
                .progress
                .set_fraction(if results.is_empty() { 0.0 } else { 1.0 });

            if !results.is_empty() {
                let result_count = results.len();
                let skipped_count = results
                    .iter()
                    .filter(|item| item.skipped_not_smaller)
                    .count();
                let exported_count = result_count - skipped_count;

                if skipped_count == result_count {
                    let already_optimized = trn(
                        "Image already optimized",
                        "Images already optimized",
                        result_count as u32,
                    );
                    content.progress_label.set_text(&format!(
                        "{}  •  {}",
                        tr("Finished"),
                        already_optimized,
                    ));
                    toast_overlay.add_toast(adw::Toast::new(&already_optimized));
                } else {
                    let before: u64 = results.iter().map(|item| item.original_bytes).sum();
                    let after: u64 = results.iter().map(|item| item.output_bytes).sum();
                    let (size_result, percent) = if before >= after {
                        let delta = before - after;
                        let percent = if before == 0 {
                            0.0
                        } else {
                            delta as f64 / before as f64 * 100.0
                        };
                        (tr_args("{size} saved", &[("size", human_bytes(delta))]), percent)
                    } else {
                        let delta = after - before;
                        let percent = if before == 0 {
                            0.0
                        } else {
                            delta as f64 / before as f64 * 100.0
                        };
                        (tr_args("{size} larger", &[("size", human_bytes(delta))]), percent)
                    };
                    let result_images = trn_args(
                        "{count} image",
                        "{count} images",
                        result_count as u32,
                        &[("count", result_count.to_string())],
                    );
                    let skipped_text = if skipped_count == 0 {
                        String::new()
                    } else {
                        let skipped = trn_args(
                            "{count} image already optimized",
                            "{count} images already optimized",
                            skipped_count as u32,
                            &[("count", skipped_count.to_string())],
                        );
                        format!("  •  {skipped}")
                    };
                    content.progress_label.set_text(&format!(
                        "{}  •  {}  •  {} ({percent:.1}%){skipped_text}",
                        tr("Finished"),
                        result_images,
                        size_result,
                    ));
                    let optimized = trn_args(
                        "Optimized {count} image",
                        "Optimized {count} images",
                        exported_count as u32,
                        &[("count", exported_count.to_string())],
                    );
                    let toast = if skipped_count == 0 {
                        optimized
                    } else {
                        let skipped = trn_args(
                            "{count} image already optimized",
                            "{count} images already optimized",
                            skipped_count as u32,
                            &[("count", skipped_count.to_string())],
                        );
                        format!("{optimized} • {skipped}")
                    };
                    toast_overlay.add_toast(adw::Toast::new(&toast));
                }
            } else {
                content.progress_label.set_text(&tr("No images were exported"));
            }

            if !errors.is_empty() {
                let error_count = errors.len();
                for (path, error) in &errors {
                    eprintln!(
                        "[Image Bench] Could not process {}: {error}",
                        path.display()
                    );
                }
                let error_message = trn_args(
                    "{count} image could not be processed",
                    "{count} images could not be processed",
                    error_count as u32,
                    &[("count", error_count.to_string())],
                );
                toast_overlay.add_toast(adw::Toast::new(&error_message));
            }

            refresh_action_state(&sidebar, &content, &state);
        });
    });
}

fn connect_basic_settings(sidebar: &SidebarUi, content: &ContentUi, state: &SharedState) {
    let custom_width_row = sidebar.custom_width_row.clone();
    let syncing_resize_ui = sidebar.syncing_resize_ui.clone();
    let sidebar_for_width = sidebar.clone();
    let content_for_width = content.clone();
    let state_for_width = state.clone();
    sidebar.width_row.connect_selected_notify(move |row| {
        if syncing_resize_ui.get() {
            return;
        }
        let preset = row.selected();
        custom_width_row.set_visible(preset == 3);
        let custom_width = custom_width_row.value() as u32;
        set_current_entry_resize(&state_for_width, preset, custom_width);
        refresh_queue_settings_labels(&content_for_width, &state_for_width);
        refresh_selected_preview(&sidebar_for_width, &content_for_width, &state_for_width);
    });

    let syncing_resize_ui = sidebar.syncing_resize_ui.clone();
    let sidebar_for_custom_width = sidebar.clone();
    let content_for_custom_width = content.clone();
    let state_for_custom_width = state.clone();
    sidebar.custom_width_row.connect_value_notify(move |row| {
        if syncing_resize_ui.get() {
            return;
        }
        if sidebar_for_custom_width.width_row.selected() == 3 {
            set_current_entry_resize(&state_for_custom_width, 3, row.value() as u32);
            refresh_queue_settings_labels(&content_for_custom_width, &state_for_custom_width);
            refresh_selected_preview(
                &sidebar_for_custom_width,
                &content_for_custom_width,
                &state_for_custom_width,
            );
        }
    });

    let custom_quality_row = sidebar.custom_quality_row.clone();
    let apply_quality_row = sidebar.apply_quality_row.clone();
    let apply_quality_button = sidebar.apply_quality_button.clone();
    let applied_custom_quality = sidebar.applied_custom_quality.clone();
    let custom_quality_dirty = sidebar.custom_quality_dirty.clone();
    let syncing_quality_ui = sidebar.syncing_quality_ui.clone();
    let suffix_row = sidebar.quality_suffix_row.clone();
    let sidebar_for_quality_preset = sidebar.clone();
    let content_for_quality_preset = content.clone();
    let state_for_quality_preset = state.clone();
    sidebar.quality_row.connect_selected_notify(move |row| {
        if syncing_quality_ui.get() {
            return;
        }

        let is_custom = row.selected() == 3;
        custom_quality_row.set_visible(is_custom);
        apply_quality_row.set_visible(is_custom);

        if is_custom {
            custom_quality_dirty.set(true);
            apply_quality_button.set_sensitive(true);
            refresh_action_state(
                &sidebar_for_quality_preset,
                &content_for_quality_preset,
                &state_for_quality_preset,
            );
            return;
        }

        custom_quality_dirty.set(false);
        apply_quality_button.set_sensitive(false);
        let preset = quality_preset_from_index(row.selected());
        set_current_entry_quality(
            &state_for_quality_preset,
            preset,
            None,
        );
        refresh_queue_settings_labels(&content_for_quality_preset, &state_for_quality_preset);
        let quality = quality_percentage(preset, None).unwrap_or(applied_custom_quality.get());
        applied_custom_quality.set(quality);
        update_quality_suffix_subtitle(row, quality, &suffix_row);
        refresh_action_state(
            &sidebar_for_quality_preset,
            &content_for_quality_preset,
            &state_for_quality_preset,
        );
        refresh_selected_preview(
            &sidebar_for_quality_preset,
            &content_for_quality_preset,
            &state_for_quality_preset,
        );
    });

    let quality_row = sidebar.quality_row.clone();
    let apply_quality_button = sidebar.apply_quality_button.clone();
    let custom_quality_dirty = sidebar.custom_quality_dirty.clone();
    let syncing_quality_ui = sidebar.syncing_quality_ui.clone();
    let sidebar_for_quality_draft = sidebar.clone();
    let content_for_quality_draft = content.clone();
    let state_for_quality_draft = state.clone();
    sidebar.custom_quality_row.connect_value_notify(move |_| {
        if syncing_quality_ui.get() {
            return;
        }
        if quality_row.selected() == 3 {
            custom_quality_dirty.set(true);
            apply_quality_button.set_sensitive(true);
            refresh_action_state(
                &sidebar_for_quality_draft,
                &content_for_quality_draft,
                &state_for_quality_draft,
            );
        }
    });

    let quality_row = sidebar.quality_row.clone();
    let custom_quality_row = sidebar.custom_quality_row.clone();
    let applied_custom_quality = sidebar.applied_custom_quality.clone();
    let custom_quality_dirty = sidebar.custom_quality_dirty.clone();
    let suffix_row = sidebar.quality_suffix_row.clone();
    let sidebar_for_apply_quality = sidebar.clone();
    let content_for_apply_quality = content.clone();
    let state_for_apply_quality = state.clone();
    sidebar.apply_quality_button.connect_clicked(move |button| {
        if quality_row.selected() != 3 {
            return;
        }

        let applied = custom_quality_row.value() as u8;
        applied_custom_quality.set(applied);
        set_current_entry_quality(
            &state_for_apply_quality,
            QualityPreset::Custom,
            Some(applied),
        );
        refresh_queue_settings_labels(&content_for_apply_quality, &state_for_apply_quality);
        custom_quality_dirty.set(false);
        button.set_sensitive(false);
        update_quality_suffix_subtitle(&quality_row, applied, &suffix_row);
        refresh_action_state(
            &sidebar_for_apply_quality,
            &content_for_apply_quality,
            &state_for_apply_quality,
        );
        refresh_selected_preview(
            &sidebar_for_apply_quality,
            &content_for_apply_quality,
            &state_for_apply_quality,
        );
    });

    let apply_settings_menu = sidebar.apply_settings_menu.clone();
    let content_for_apply_selected = content.clone();
    let state_for_apply_selected = state.clone();
    sidebar
        .apply_selected_settings_button
        .connect_clicked(move |_| {
            let targets: Vec<usize> = {
                let app_state = state_for_apply_selected.borrow();
                app_state
                    .entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        app_state.bulk_selected.contains(&entry.path).then_some(index)
                    })
                    .collect()
            };
            let applied = apply_current_settings_to_indices(&state_for_apply_selected, &targets);
            refresh_queue_settings_labels(&content_for_apply_selected, &state_for_apply_selected);
            if applied > 0 {
                eprintln!(
                    "[Image Bench] Applied current Resize + Quality settings to {applied} selected image(s)"
                );
            }
            apply_settings_menu.popdown();
        });

    let apply_settings_menu = sidebar.apply_settings_menu.clone();
    let content_for_apply_all = content.clone();
    let state_for_apply_all = state.clone();
    sidebar.apply_all_settings_button.connect_clicked(move |_| {
        let targets: Vec<usize> = {
            let app_state = state_for_apply_all.borrow();
            (0..app_state.entries.len()).collect()
        };
        let applied = apply_current_settings_to_indices(&state_for_apply_all, &targets);
        refresh_queue_settings_labels(&content_for_apply_all, &state_for_apply_all);
        if applied > 0 {
            eprintln!(
                "[Image Bench] Applied current Resize + Quality settings to {applied} image(s)"
            );
        }
        apply_settings_menu.popdown();
    });

    let sidebar_for_mode = sidebar.clone();
    let content_for_mode = content.clone();
    let state_for_mode = state.clone();
    content.preview_modes.connect_active_notify(move |_| {
        refresh_selected_preview(&sidebar_for_mode, &content_for_mode, &state_for_mode);
        refresh_zoom_state(&content_for_mode);
    });

    let content_for_zoom = content.clone();
    content.zoom_row.connect_selected_notify(move |_| {
        // The wheel sets its own cursor anchor first; only the dropdown path
        // reaches here without one, and that anchors on the viewport centre.
        if content_for_zoom.zoom_anchor.get().is_none() {
            capture_zoom_anchor(&content_for_zoom, None);
        }
        apply_preview_zoom(&content_for_zoom);
    });

    // One connection covers every set_paintable call site, so a re-render or an
    // Original/Preview switch keeps the selected zoom level.
    let content_for_paintable = content.clone();
    content.preview.connect_paintable_notify(move |_| {
        apply_preview_zoom(&content_for_paintable);
    });

    let content_for_compare_paintable = content.clone();
    content
        .compare
        .picture
        .connect_paintable_notify(move |_| {
            apply_preview_zoom(&content_for_compare_paintable);
        });

    connect_preview_navigation(content);

    let suffix_entry = sidebar.filename_suffix_entry.clone();
    sidebar.filename_suffix_check.connect_toggled(move |check| {
        suffix_entry.set_visible(check.is_active());
    });
}

fn quality_settings(
    row: &adw::ComboRow,
    custom_quality: u8,
) -> (QualityPreset, Option<u8>) {
    match row.selected() {
        0 => (QualityPreset::High, None),
        1 => (QualityPreset::Balanced, None),
        2 => (QualityPreset::Compact, None),
        _ => (QualityPreset::Custom, Some(custom_quality)),
    }
}

fn update_quality_suffix_subtitle(
    row: &adw::ComboRow,
    custom_quality: u8,
    suffix_row: &adw::ActionRow,
) {
    let (preset, custom_quality) = quality_settings(row, custom_quality);
    if let Ok(percentage) = quality_percentage(preset, custom_quality) {
        suffix_row.set_subtitle(&tr_args(
            "Adds -{percentage} for the current quality setting",
            &[("percentage", percentage.to_string())],
        ));
    }
}

fn build_content() -> ContentUi {
    let queue_split = adw::OverlaySplitView::new();
    queue_split.set_sidebar_position(gtk::PackType::End);
    queue_split.set_sidebar_width_fraction(0.30);
    queue_split.set_min_sidebar_width(280.0);
    queue_split.set_max_sidebar_width(360.0);
    queue_split.set_show_sidebar(false);

    // Central work area: its header belongs only to the preview/content pane.
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_centering_policy(adw::CenteringPolicy::Strict);

    let preview_modes = adw::ToggleGroup::new();
    preview_modes.add_css_class("image-bench-preview-modes");
    let original_toggle = adw::Toggle::new();
    original_toggle.set_label(Some(&tr("Original")));
    preview_modes.add(original_toggle);
    let compare_toggle = adw::Toggle::new();
    compare_toggle.set_label(Some(&tr("Compare")));
    preview_modes.add(compare_toggle);
    let preview_toggle = adw::Toggle::new();
    preview_toggle.set_label(Some(&tr("Preview")));
    preview_modes.add(preview_toggle);
    preview_modes.set_active(2);
    preview_modes.set_sensitive(false);
    header.set_title_widget(Some(&preview_modes));
    let sidebar_button = gtk::ToggleButton::builder()
        .icon_name("panel-left-symbolic")
        .tooltip_text(tr("Show sidebar"))
        .visible(true)
        .build();
    sidebar_button.connect_toggled(|button| {
        button.set_tooltip_text(Some(&tr(if button.is_active() {
            "Hide sidebar"
        } else {
            "Show sidebar"
        })));
    });
    header.pack_start(&sidebar_button);

    let queue_button = gtk::ToggleButton::builder()
        .icon_name("panel-right-symbolic")
        .tooltip_text(tr("Show batch queue"))
        .build();
    queue_button.set_visible(true);
    queue_button.set_sensitive(false);
    queue_button.connect_toggled(|button| {
        button.set_tooltip_text(Some(&tr(if button.is_active() {
            "Hide batch queue"
        } else {
            "Show batch queue"
        })));
    });
    header.pack_end(&queue_button);

    let fit_label = tr("Fit");
    let zoom_model = gtk::StringList::new(&[fit_label.as_str(), "100%", "200%", "400%"]);
    let zoom_row = gtk::DropDown::builder()
        .model(&zoom_model)
        .selected(0)
        .tooltip_text(tr("Preview zoom"))
        .sensitive(false)
        .build();
    zoom_row.add_css_class("flat");
    header.pack_end(&zoom_row);
    let pointer: Rc<Cell<(f64, f64)>> = Rc::new(Cell::new((0.0, 0.0)));
    let zoom_anchor: Rc<Cell<Option<(f64, f64, f64, f64)>>> = Rc::new(Cell::new(None));
    toolbar.add_top_bar(&header);

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    toolbar.set_content(Some(&stack));

    let empty_page = adw::StatusPage::builder()
        .icon_name(config::app_id())
        .title(tr("Drop images here"))
        .description(tr("Drop one or more JPEG or PNG images here, or use Add Images / Add Folder."))
        .vexpand(true)
        .build();
    stack.add_named(&empty_page, Some("empty"));

    let preview_box = gtk::Box::new(Orientation::Vertical, 0);
    preview_box.add_css_class("image-bench-workspace");
    preview_box.set_hexpand(true);
    preview_box.set_vexpand(true);
    stack.add_named(&preview_box, Some("queue"));

    let preview_frame = gtk::Box::new(Orientation::Vertical, 8);
    preview_frame.set_hexpand(true);
    preview_frame.set_vexpand(true);
    preview_frame.set_margin_top(18);
    preview_frame.set_margin_start(18);
    preview_frame.set_margin_end(18);
    preview_frame.set_margin_bottom(18);

    let preview_display = gtk::Stack::new();
    preview_display.set_transition_type(gtk::StackTransitionType::Crossfade);
    preview_display.set_hexpand(true);
    preview_display.set_vexpand(true);
    preview_display.set_margin_top(12);
    preview_display.set_margin_start(12);
    preview_display.set_margin_end(12);

    let preview = gtk::Picture::new();
    preview.set_content_fit(gtk::ContentFit::Contain);
    preview.set_can_shrink(true);
    preview.set_hexpand(true);
    preview.set_vexpand(true);

    // The scroller is the zoom viewport. In Fit it never scrolls; above Fit it
    // provides native scrollbars, wheel and touch panning without competing
    // with the Compare drag gesture.
    let preview_scroller = gtk::ScrolledWindow::new();
    preview_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    preview_scroller.set_hexpand(true);
    preview_scroller.set_vexpand(true);
    preview_scroller.set_child(Some(&preview));
    preview_display.add_named(&preview_scroller, Some("single"));

    let compare = CompareView::new();
    compare.root.set_tooltip_text(Some(&tr("Drag horizontally to compare Original and Preview")));
    preview_display.add_named(&compare.scroller, Some("compare"));
    preview_display.set_visible_child_name("single");

    let preview_overlay = gtk::Overlay::new();
    preview_overlay.set_hexpand(true);
    preview_overlay.set_vexpand(true);
    preview_overlay.set_child(Some(&preview_display));
    let preview_spinner = gtk::Spinner::new();
    preview_spinner.set_halign(Align::Center);
    preview_spinner.set_valign(Align::Center);
    preview_spinner.set_size_request(32, 32);
    preview_spinner.set_visible(false);
    preview_overlay.add_overlay(&preview_spinner);
    preview_frame.append(&preview_overlay);

    let preview_title = gtk::Label::builder().xalign(0.5).halign(Align::Center).build();
    preview_title.add_css_class("heading");
    preview_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    preview_title.set_margin_start(12);
    preview_title.set_margin_end(12);
    preview_frame.append(&preview_title);

    let preview_meta = gtk::Label::builder().xalign(0.5).halign(Align::Center).build();
    preview_meta.add_css_class("dim-label");
    preview_meta.set_margin_start(12);
    preview_meta.set_margin_end(12);
    preview_meta.set_margin_bottom(12);
    preview_frame.append(&preview_meta);
    preview_box.append(&preview_frame);
    queue_split.set_content(Some(&toolbar));

    // Right queue sidebar: its own ToolbarView keeps its header inside the sidebar.
    let queue_toolbar = adw::ToolbarView::new();
    let queue_header = adw::HeaderBar::new();
    let queue_header_spacer = gtk::Box::new(Orientation::Horizontal, 0);
    queue_header.set_title_widget(Some(&queue_header_spacer));
    let queue_title = gtk::Label::builder()
        .label(tr("Batch queue"))
        .xalign(0.0)
        .build();
    queue_title.add_css_class("heading");
    queue_title.set_margin_start(6);
    queue_header.pack_start(&queue_title);
    let initial_queue_count = tr("0 images");
    let queue_count_label = gtk::Label::new(Some(&initial_queue_count));
    queue_count_label.add_css_class("dim-label");
    queue_count_label.set_margin_end(6);
    queue_header.pack_end(&queue_count_label);
    queue_toolbar.add_top_bar(&queue_header);

    let queue_panel = gtk::Box::new(Orientation::Vertical, 0);
    queue_panel.set_vexpand(true);

    let queue_scroller = gtk::ScrolledWindow::new();
    queue_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    queue_scroller.set_vexpand(true);
    queue_scroller.set_margin_start(6);
    queue_scroller.set_margin_end(6);
    queue_panel.append(&queue_scroller);

    let listbox = gtk::ListBox::new();
    listbox.set_selection_mode(gtk::SelectionMode::Single);
    listbox.set_activate_on_single_click(true);
    let queue_settings_labels = Rc::new(RefCell::new(Vec::new()));
    listbox.set_show_separators(false);
    listbox.add_css_class("navigation-sidebar");
    queue_scroller.set_child(Some(&listbox));

    let progress_box = gtk::Box::new(Orientation::Vertical, 6);
    progress_box.set_margin_start(6);
    progress_box.set_margin_end(6);
    progress_box.set_margin_bottom(18);
    let progress_label = gtk::Label::builder().xalign(0.0).build();
    progress_label.add_css_class("dim-label");
    progress_label.set_wrap(true);
    let progress = gtk::ProgressBar::new();
    progress_box.append(&progress_label);
    progress_box.append(&progress);
    progress_box.set_visible(false);
    queue_panel.append(&progress_box);

    queue_toolbar.set_content(Some(&queue_panel));
    queue_split.set_sidebar(Some(&queue_toolbar));

    stack.set_visible_child_name("empty");

    ContentUi {
        sidebar_button,
        queue_button,
        queue_split,
        preview_modes,
        stack,
        preview_display,
        preview_spinner,
        preview_scroller,
        preview,
        zoom_row,
        pointer,
        zoom_anchor,
        compare,
        preview_title,
        preview_meta,
        queue_count_label,
        listbox,
        queue_settings_labels,
        progress_box,
        progress_label,
        progress,
    }
}


