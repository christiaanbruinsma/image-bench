## Candidate 52

- Align sidebar icon handling with the Data Inspector Golden Standard: bundle the exact `panel-left-symbolic` and `panel-right-symbolic` hicolor action fallbacks used by Data Inspector while keeping the semantic GTK icon names unchanged.

## Candidate 51

- Removed bundled sidebar icon fallbacks.
- Left and right sidebar toggles now rely exclusively on the native GTK icon names `panel-left-symbolic` and `panel-right-symbolic`.

## Candidate 49 — persistent sidebar toggles

## Candidate 50 — official sidebar icon fallback

- Use `panel-left-symbolic` and `panel-right-symbolic` directly for the persistent sidebar toggles.
- Remove directional `pan-start-symbolic` / `pan-end-symbolic` fallbacks from the sidebar buttons.
- Bundle minimal symbolic fallbacks under the same official panel icon names in `hicolor/scalable/actions`, so GTK still prefers the active user icon theme and only falls back when those names are unavailable.

- Keep the left settings and right batch-queue toggle buttons permanently visible in the central header, including wide layouts.
- Preserve the existing bidirectional bindings to each `AdwOverlaySplitView`; no new sidebar state machine is introduced.
- Keep responsive breakpoints responsible only for collapsed layout behavior, not for hiding the controls.
- Add translated Show/Hide tooltips that follow each toggle button's active state.
- No Compare, image-processing, GPU, export, queue-data, or preset behavior changed.

## Candidate 47 — unified Compare control

## Candidate 48

- Restored the queue-item dismiss icon to `window-close-symbolic`; `list-remove-symbolic` remains only as a semantic fallback.

- Replace the separately positioned Compare divider and drag handle with one moving `GtkOverlay` CompareControl.
- Keep the divider and drag handle as centered children of that single control, so they share one parent allocation and cannot drift apart horizontally.
- Derive the Original/Preview clip boundary from the exact center of the same CompareControl allocation instead of from an independent position formula.
- Preserve the proven 44 px centered handle/icon layout, single fixed-overlay `GestureDrag`, native symbolic icons, and C45 diagnostics for one final runtime verification.
- No image processing, GPU, export, sidebar, translation, or icon-theme behavior changed.

## Candidate 46 — Compare geometry fix

- Fix Compare drag geometry from runtime evidence: logical position now maps to the handle travel range (`image width - handle width`) instead of the full image width.
- Divider still spans the complete image width while the 44 px handle remains fully inside the image, so both reach their correct left/right boundaries at the same logical 0%/100% positions.
- Replace the drag handle `GtkGrid` with `GtkCenterBox`; the two native symbolic pan icons are now the actual center widget instead of receiving a top-left grid allocation.
- Keep the C45 Compare diagnostics for one verification run before release hardening.
- No image processing, GPU, export, sidebar, translation, or icon-theme behavior changed.

## Candidate 45 — diagnostic only — 2026-08-10

- Add read-only Compare geometry diagnostics; no Compare allocation, CSS, drag-update, rendering, clipping, processing, or GPU/cache behavior is changed.
- Log source/preview texture dimensions and aspect ratio when Compare textures are installed.
- Log root/overlay/picture/divider/handle bounds at drag begin and drag end, all measured in the Compare overlay coordinate space.
- Log handle icon-cluster and individual symbolic-icon bounds in the handle coordinate space to prove or disprove geometric centering.
- Preserve the real drag origin and log final drag offsets/pointer X so the current pointer-to-divider mapping can be verified from runtime evidence.

## Candidate 44 — 2026-08-09

- Restyled Resize, Quality, and Output information indicators as 36×36 non-clickable flat icon controls with centered icons, rounded-square hover surfaces, and smooth hover transitions.
- Increased information-control spacing from the sidebar edge.
- Removed manual section dividers from the left settings sidebar and row separators from the right queue sidebar for a quieter layout.

## Candidate 43

- Replace the legacy Image Bench application icon with the suite-standard 128×128 vector baseline: exact 112×112 rounded blue GNOME-style background, `#3584e4` → `#1c71d8` diagonal gradient, and a single image-resize metaphor in white/light-blue geometry.
- Add a true 16×16 `io.github.christiaanbruinsma.ImageBench-symbolic.svg` using the same image-resize metaphor as a simplified monochrome line icon.
- Keep both source SVG assets under `data/icons/` and install them through Meson into the hicolor scalable and symbolic application icon directories, renamed to the active application ID for stable/Devel builds.
- Remove the old duplicate icon source from `data/icons/hicolor/...`; no processing, UI, GPU/cache, translation, or Compare behavior changes.

## Candidate 42

- Replace the Compare GtkButton handle with a fixed-size non-button GtkGrid visual handle so GTK button padding/theme metrics can no longer shift or enlarge the drag control.
- Center the semantic `pan-start-symbolic` + `pan-end-symbolic` icon pair explicitly inside the 44 px handle and preserve accent hover feedback without click/button semantics.
- Clip both divider and handle explicitly to the Compare overlay and keep their allocations in the same image coordinate space, while preserving the single overlay GestureDrag input path.
- Enlarge Resize/Quality/Output info indicators to 32 px non-clickable hover targets with centered semantic icons, 8 px right spacing, subtle theme-aware hover surface, and native tooltips.
- Keep the icon-standard cleanup from Candidate 41, including semantic queue/sidebar/menu icons and active-icon-theme resolution without theme-specific hardcoding.
- Preserve translations, loader, processing, GPU/cache behavior, per-image settings, and queue UX unchanged.

## Candidate 41

- Rebuild Compare geometry around a native GtkAspectFrame so the Compare overlay itself is the exact painted image rectangle; divider and handle no longer reconstruct image bounds from a larger workspace allocation.
- Keep the divider at the exact image edges while clamping the circular handle fully inside the image and preserving the single fixed-overlay drag gesture.
- Replace hardcoded Compare/remove action glyphs with semantic themed GTK/Freedesktop icon names.
- Use `list-remove-symbolic` for queue removal, `edit-clear-symbolic` for Clear Queue, `pan-start-symbolic` + `pan-end-symbolic` for the Compare handle, and the suite-standard `panel-left-symbolic` / `panel-right-symbolic` for sidebar controls.
- Resolve suite-specific sidebar icons against the active GtkIconTheme at runtime and fall back only to semantic GTK pan-start/pan-end icons, avoiding missing-icon placeholders and theme-specific hardcoding.
- Remove the semantically incorrect `help-about-symbolic` and text `i` fallbacks from info indicators; use semantic themed information/help icons only.
- Preserve Candidate 40 loader, centered mode switcher, translations, processing, GPU/cache behavior, and per-image settings unchanged.

## Candidate 40

- Replace the redundant Preview header title with the Original/Compare/Preview ToggleGroup as the strictly centered HeaderBar title widget.
- Add subtle theme-aware accent hover feedback to the non-clickable Resize, Quality, and Output info indicators while preserving native GTK tooltips and no click affordance.
- Add a centered native GtkSpinner overlay for real Preview/Compare/Original processing, delayed by 150 ms to avoid flashing on fast operations and kept active across pending latest-render jobs.
- Preserve cache-hit mode switching without any loader, and keep Candidate 39 Compare geometry, translations, processing, GPU/cache behavior, and per-image settings unchanged.

## Candidate 39

- Clamp the Compare drag handle against GTK's measured natural button size instead of the 36 px minimum request, keeping the full native handle inside the image at both horizontal edges while the divider still reaches 0%/100%.
- Replace Resize and Quality clickable info buttons with non-interactive theme icons carrying native GTK tooltips.
- Move the Output safety copy into the same non-interactive info-tooltip pattern for a consistent compact sidebar.
- Preserve Candidate 38 separators, Clear Queue hover styling, active Preview-mode accent styling, translations, processing, GPU/cache behavior, and per-image settings unchanged.

## Candidate 38

- Give Clear Queue the native destructive-action styling only while hovered, preserving its neutral default state.
- Style the active Original/Compare/Preview toggle with the current Adwaita accent background/foreground via the ToggleGroup checked state.
- Move Resize and Quality explanatory copy into compact header-suffix info buttons with translated tooltips.
- Add native horizontal GtkSeparator widgets between Images, Resize, Quality, and Output sections while preserving the Output safety description.
- Preserve Candidate 37 processing, GPU/cache behavior, per-image settings, queue UX, and translation catalogs unchanged.

## Candidate 37

- Freeze the English v0.9.0 user-facing strings and add GNU gettext runtime localization through `gettext-rs` using the system gettext implementation.
- Add Meson `i18n.gettext()` integration, locale installation under the Flatpak prefix, a source POT, and complete NL/DE/FR/ES/IT/PT PO catalogs.
- Localize runtime UI labels, dialogs, tooltips, queue summaries, Preview/Compare status text, optimization progress/toasts, and plural image counts while keeping product/technical identifiers stable.
- Add localized desktop comments/keywords and AppStream summaries/descriptions/release text for NL/DE/FR/ES/IT/PT.
- Preserve Candidate 36 processing, GPU/cache behavior, per-image Resize/Quality state, queue UX, and Compare geometry unchanged.

## Candidate 36

- Extend the Compare divider to the full painted image height using floor/ceil image bounds so no 1 px gap remains at fractional vertical centering positions.
- Keep the Compare divider itself able to reach the exact left/right image edge while clamping the circular handle fully inside the visible image bounds.
- Preserve Compare drag behavior, GPU/render caches, per-image Resize/Quality state, and Candidate 35 queue UX.

## Candidate 35

- Replace queue multi-selection as the Current Image indicator with native single-row selection/highlight; the highlighted row is the image shown in Preview and edited in the sidebar.
- Add an explicit checkbox to every queue item for bulk-target selection, separating Current Image from Apply-to-Selected semantics.
- Add a live per-image settings line in the queue showing `Resize: <value> px • Quality: <value>%`.
- Keep queue settings summaries synchronized after per-image Resize/Quality changes and after Apply to Selected/All without rebuilding thumbnails.
- Preserve per-image processing state, render/GPU caches, batch export semantics, and the Compare edge polish from Candidate 34.

## Candidate 34

- Store Resize settings per image alongside the existing per-image Quality settings; selecting the Current Image restores both Resize and Quality in the sidebar.
- Batch Optimize now uses each image's own target width as well as its own Quality settings.
- Enable native multi-selection in the image queue while keeping an explicit Current Image as the Preview/sidebar settings source; plain click changes Current, while Ctrl/Shift selection only changes the bulk target set.
- Add “Apply current settings” actions to copy the Current Image's Resize + Quality to Selected Images or All Images.
- Keep Custom Quality draft/apply semantics authoritative: bulk settings actions stay disabled until a pending Custom Quality value is applied.
- Preserve the Candidate 33 Compare handle edge behavior and the C32 render/GPU caches.

## Candidate 33

- Compare handle can now stay centered on the divider at the exact left/right image edge.
- Quality settings are now stored per image instead of globally across the queue.
- Selecting an image restores its own Quality preset/custom value in the sidebar.
- Custom Quality Apply updates only the selected image.
- Batch Optimize uses each image's own Quality settings.

## Candidate 32

- Add an active-image render cache so Preview, Compare, and Original reuse already generated textures instead of reprocessing on view-mode switches.
- Cache preview texture + output metadata by source path, target width, quality preset, and applied custom quality; filename/output settings do not invalidate the visual preview.
- Materialize and cache the Original GDK texture from the existing decoded RawImage instead of decoding the source again through Glycin for Original/Compare.
- Keep C30 serialized preview jobs and C31 GPU-ready source texture cache unchanged; processing runs again only when preview-affecting settings change.

### Candidate 31
- Cache one GPU-ready source texture so repeated Preview/Compare renders for the active large image skip repeated RGB→RGBA conversion and source uploads.
- Replace the GPU source cache when a different source image is processed; cache growth remains bounded to one source texture.
- Split GPU resize timings into source-cache hit/miss, input preparation, texture setup, upload, GPU work, readback unpack, output packing, and total path time.
- Keep C30 Custom Quality Apply semantics, serialized preview jobs, GPU shader, CPU fallback, and JPEG/PNG output behavior unchanged.

### Candidate 30
- Add a native full-width Apply row to Custom Quality so draft quality changes do not trigger preview rendering until explicitly applied.
- Keep the last applied Custom Quality as the single source of truth for Preview and Optimize; disable Optimize while a Custom Quality draft is pending.
- Serialize heavy Preview/Compare work to at most one active job plus one latest pending rerender, preventing rapid settings changes from queueing many concurrent GPU/CPU renders.
- Revalidate preview generation on decoded-source cache hits so stale cached jobs stop before expensive processing.

### Candidate 29
- Fix the wgpu 30 `PipelineLayoutDescriptor` API usage: optional bind-group layout entries and `immediate_size: 0`.

### Candidate 28
- Add the first experimental GPU resize path for large images using a reusable wgpu/WGSL bilinear compute pipeline.
- Keep the CPU resize path as an automatic fallback when GPU resize is unavailable, unsuitable, or fails.
- Add GPU resize timing evidence for upload enqueue, GPU work/readback transfer, CPU readback conversion, and total GPU path time.
- Use a temporary 12 MP prototype threshold; the production CPU/GPU dispatch threshold remains benchmark-driven.
### Candidate 27
- Add a probe-only `wgpu 30.0.0` GPU foundation using Vulkan + WGSL on Linux; no pixels are sent to the GPU yet.
- Initialize a high-performance physical adapter plus persistent `Device` and `Queue` asynchronously at application startup.
- Log adapter/backend/device/driver details as `[Image Bench GPU]` evidence for the next GPU-resize gate.
- Reject software CPU adapters and fail closed to the existing `ResizeBackend::Cpu` path when adapter/device initialization is unavailable.
- Keep C26 decoded-source caching, JPEG/PNG processing, Preview/Compare behavior, and export semantics unchanged.

### Candidate 26
- Add a single active-image decoded source cache so Preview and Compare rendering can reuse the already decoded `RawImage` instead of decoding the same large source again on every settings change.
- Seed the cache from the first successfully imported image without retaining decoded buffers for the whole batch.
- Add an explicit `ImageProcessor` / `ResizeBackend::Cpu` boundary ahead of the GPU backend phase.
- Change CPU resize input to a borrowed `fast_image_resize::images::ImageRef`, so cached source pixels do not need to be copied or consumed before resizing.
- Add cache/backend performance markers (`stage=source-cache`, `decoded_source`, `resize_backend`) without changing JPEG/PNG encoding semantics or the no-larger JPEG invariant.

### Candidate 25
- Keep the Compare divider neutral while making the circular Compare handle use the Adwaita accent background on hover/press with the accent foreground icon.
- Restore pointer targeting only for the handle so native hover works; the single fixed-overlay `GtkGestureDrag` remains the sole drag controller.

## Rust Candidate 23
### Candidate 24
- Matched the Compare divider and handle to the central workspace surface using the Adwaita window background/foreground tokens.
- Kept the Compare handle visually solid across pointer states by making the moving handle pointer-transparent; the fixed overlay remains the sole drag controller.


- Fix Compare drag jitter by using one `GtkGestureDrag` on the fixed `GtkOverlay` as the sole drag coordinate space.
- Replace the snapshot-drawn dual-stroke divider with an exact 2 px overlay divider.
- Style both divider and circular handle with the native libadwaita `.view` colors so they form one solid theme-adaptive control: light in light mode, dark in dark mode, with matching foreground contrast.
- Remove translucent `.osd` styling from the Compare handle; processing and performance probes are unchanged.

## Rust Candidate 21

- Candidate 22: fix Candidate 21 compile regression by restoring `ContentUi` and making Compare overlay state types explicit; no behavior changes.
- Replace the custom-drawn Compare grab block with a native GTK overlay button using libadwaita `circular` + `osd` styling and native hover/pressed feedback.
- Keep the Compare divider positioned by `GtkOverlay::get-child-position`, including during window resizing and at the image edges.
- Add development performance timings for raw decode, CPU resize, encode, generated-preview decode, and total Preview/Compare latency.
- Keep processing behavior unchanged; this candidate measures the existing CPU pipeline before GPU backend work.

## Rust Candidate 20

- Rename the user-facing compression semantics to quality semantics: `3. Quality`, `Quality level`, `High — 92%`, `Balanced — 85%`, `Compact — 75%`, and `Custom`.
- Rename the optional compression suffix to a quality suffix so values such as `-85` are not presented as compression percentages.
- Add native `Original | Compare | Preview` modes to the central Preview header.
- Add an in-memory Compare view with Original on the left and the exact generated Preview on the right.
- Add a draggable vertical before/after divider that updates only the presentation clip while dragging; no re-encode occurs during slider movement.
- Keep Compare on the same resize, quality, adaptive JPEG no-larger, and stale-generation pipeline as Preview/export.

## Rust Candidate 19

- Enforce a hard JPEG invariant: optimized output is never larger than the source, including resized exports.
- Treat the selected JPEG quality as the maximum requested quality and search downward when that encoding would exceed the source byte size.
- Use the highest fitting quality found by the bounded search and keep Preview on the exact same encoded result as export.
- Fall back to the original bytes and skip export when no JPEG quality in the supported range can satisfy the source-size limit.
- Use the effective JPEG quality in optional compression suffixes when automatic quality reduction was required.

## Rust Candidate 7

- Add a no-larger guard for JPEG/JPG files that do not require resizing.
- Skip writing a re-encoded JPEG when it would be the same size or larger than the source.
- Report skipped files as already optimized instead of as failed or enlarged exports.
- Preserve existing export behavior when an actual resize is requested.

## Rust Candidate 5

- Fix Rust build failure in resize worker error mapping (`spawn_blocking` panic payload is not `Display`).

- Enable Optimize only when images and an output folder are available and no import/process task is active.
- Use a singular **Optimize Image** label for one queued image and **Optimize Images** for multiple images.
- Connect the Optimize action to the Rust batch processor with progress, per-file error handling, aggregate savings, and completion toasts.
- Run CPU-bound bilinear resizing through `gio::spawn_blocking` to keep the GTK main loop responsive.

## Rust Candidate 4

- Fix the development empty-state icon by resolving the active application ID instead of hardcoding the production ID.
- Wire the Output folder Choose button to the native GTK FileDialog folder chooser.
- Store the selected output directory in application state and show it in the Output row.

## Rust Candidate 3

- Add selected-image preview via Glycin in the Rust UI.
- Auto-select the first newly imported image.
- Preserve the existing source-to-target dimension preview and stale-preview guard.
- Refresh preview metadata when resize settings change.

## Rust Candidate 2

- Fix Glycin `Cancellable` ownership at the loader and creator boundaries by cloning the borrowed GIO object before passing it to APIs that require `impl IsA<gio::Cancellable>`.

## Candidate 10

- Group advanced filename/output suffix controls under a native libadwaita `Adw.ExpanderRow`.
- Keep advanced output options collapsed by default while leaving the output-folder chooser always visible.

## Candidate 9

- Add optional editable filename suffix, enabled by default as `-optimized`.
- Add optional compression percentage suffix, disabled by default.
- Keep suffixes before collision counters and never overwrite existing output.
- Remove accumulated candidate validation files from the source tree.

## Candidate 8

- Restore the suite-standard single application menu in the sidebar header.
- Group About Image Bench and Quit in that menu.
- Remove the redundant content-header Quit menu.

## Candidate 7

- Replace the Batch queue generic image theme icon with a real 64 px thumbnail derived from the decoded image.
- Check remaining theme icon names against the active Gtk.IconTheme before use.
- Fall back to text labels instead of ever showing a broken theme-icon placeholder.

## Candidate 6

- Fix GNOME Builder/Glycin development execution by using the documented `.Devel` Flatpak app-ID convention.
- Restore Glycin-only source decoding after proving GdkPixbuf delegates to the same failing Glycin loader in GNOME 50.
- Hide the sidebar reveal control in wide mode and use a safe directional symbolic icon when collapsed.

## Candidate 5

- Add controlled GdkPixbuf compatibility fallback when Glycin fails on supported JPEG/PNG files.
- Improve decode error message to include both Glycin and fallback failures when both fail.
- Replace missing header/queue symbolic icons with runtime-safe alternatives.

# Changelog
### Candidate 3

- Show the exact quality percentage in every compression preset.
- Add a Custom compression option with a 1–100% quality control.
- Preserve PNG as lossless output while mapping custom quality to compression effort.


## 0.9.0 — 8 August 2026

Initial public beta baseline.

Candidate 2 UI consistency patch:

- Move About to the right side of the Image Bench title in the sidebar header.
- Install and use a dedicated Image Bench application icon.
- Add vertical spacing between Queue and the Add Images / Add Folder controls.

- Add multi-image and folder import for JPEG/PNG.
- Add 1920, 1280 and 800 px width presets plus custom width.
- Preserve aspect ratio and prevent upscaling.
- Add Light, Balanced and Strong compression presets.
- Add selected-image preview and batch queue.
- Add explicit output-folder selection and collision-safe export.
- Add batch progress and aggregate space-saved result.
- Keep all processing local with no network dependency.

## Candidate 4

- Add multi-file drag and drop to the right content area using GTK4 `Gdk.FileList`.
- Clarify the empty state as a drop target.
- Retry Glycin decoding from in-memory source bytes when the path-backed loader fails.
- Preserve Glycin as the only source image decoder and expose concrete decode failures in runtime logs.
### Rust Candidate 8
- Restores real image thumbnails in the batch queue using the existing Glycin texture decode path.
- Keeps thumbnail loading asynchronous and separate from preview/export behavior.

### Rust Candidate 10
- Replaces the temporary `Gtk.Paned` queue layout with a native right-side `Adw.OverlaySplitView`.
- Places the batch queue at `Gtk.PackType::End` with native sidebar sizing.
- Adds a compact Queue toggle that appears when the right sidebar collapses on narrower windows.
- Keeps preview, queue state, thumbnails, processing, and export logic unchanged.

### Rust Candidate 11
- Corrects the right Batch queue to use its own `Adw.ToolbarView` and `Adw.HeaderBar`.
- Keeps the central Image Bench header scoped to the preview pane instead of spanning across the right sidebar.
- Preserves full-work-area drag and drop and adaptive queue sidebar behavior.

### Rust Candidate 12
- Removes the extra preview card background so the central preview sits directly on the work area.
- Replaces the boxed batch queue styling with native Gtk.ListBox row separators for a cleaner right sidebar.
- Keeps selection highlighting, thumbnails, queue behavior, and processing unchanged.

### Rust Candidate 13
- Removes the inset queue-list surface so the Batch queue reads as one continuous right-sidebar surface.
- Uses GTK's native `navigation-sidebar` presentation for the selectable queue list while retaining separators.
- Removes the unused `ContentUi.toolbar` field that produced a Rust dead-code warning.

### Rust Candidate 14
- Adds horizontal breathing room to the right Batch queue sidebar.
- Gives every queue row a fixed 72 × 56 thumbnail cell so filenames and metadata align consistently regardless of source aspect ratio.
- Uses `Contain` for queue thumbnails so portrait, landscape, and square images remain fully visible without cropping.
- Centers the selected image filename and metadata beneath the main preview.

### Rust Candidate 15
- Makes the left settings flow explicit with numbered sections: 1. Images, 2. Resize, 3. Compression, 4. Output.
- Aligns the Batch queue header content with the same 12 px horizontal inset used by the queue list.

## Candidate 16
- Makes batch remove actions a fixed 36×36 square for consistent row alignment.
- Keeps remove actions visually quiet by default and applies libadwaita destructive styling only while hovered.

### Rust Candidate 17
- Renames the central work-area header to `Preview`.
- Removes the redundant automatic app title from the Batch queue header.
- Aligns the right-sidebar content gutter with the left sidebar.
- Uses a compact icon-only remove action with the existing hover-only destructive styling.

### Rust Candidate 18
- Adds a native `Original | Preview` mode switch to the central Preview header.
- Generates Preview entirely in memory through the same resize and Glycin encode path used by export, then decodes the actual encoded bytes for display.
- Shows the real predicted output dimensions, encoded size, and percentage smaller/larger before export.
- Regenerates Preview when resize or compression settings change while rejecting stale asynchronous results.
- Refactors export and live preview to share one encode pipeline; no temporary preview file is written.
