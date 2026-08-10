# Image Bench

Image Bench is a local-first GNOME utility for batch image resizing and optimization.

## v0.9.0 baseline

- Add multiple JPEG/PNG images.
- Drag and drop one or multiple images into the main content area.
- Import all supported images from a selected folder.
- Resize by width while **always preserving aspect ratio**.
- Built-in width presets: **1920 px**, **1280 px**, **800 px**, plus **Custom**.
- Never upscale images that are already smaller than the selected target width.
- Quality presets: **High — 92%**, **Balanced — 85%**, **Compact — 75%**, plus **Custom 1–100%**.
- Batch processing with progress and total space saved.
- Live in-memory **Original / Compare / Preview** inspection before export using the same resize and encode pipeline as final output. Compare provides a draggable vertical before/after divider.
- Explicit output-folder selection through the native GTK file dialog.
- Optional editable filename suffix, enabled by default as `-optimized`.
- JPEG exports are never allowed to exceed the source byte size, including resized output. If the selected quality would create a larger JPEG, Image Bench automatically searches downward for a fitting quality; if no valid quality fits, export is skipped.
- Optional quality suffix, disabled by default (for example `-85`).
- Collision-safe export: source files and existing output files are never overwritten.
- Local processing only; no accounts, uploads, tracking or external web service.
- Localized UI and metadata: **English** source/fallback plus **Nederlands, Deutsch, Français, Español, Italiano, Português** via GNU gettext.

## Layout

- **Left sidebar:** import, resize, quality and output controls.
- **Right content:** selected-image preview and batch queue.
- Adaptive `Adw.OverlaySplitView` for narrower window sizes.

## Image pipeline

The first baseline supports JPEG/JPG and PNG and keeps the original file format.

Source files are decoded with **Glycin** into RGB/RGBA pixel buffers. Proportional resizing is performed on Send-safe raw buffers with **fast_image_resize** using bilinear interpolation. The resized pixels are encoded again with Glycin.

### Quality mapping

| Preset | JPEG quality | PNG compression |
|---|---:|---:|
| High | 92 | 40 |
| Balanced | 85 | 65 |
| Compact | 75 | 90 |

Custom quality accepts **1–100%**. For JPEG, the selected preset/custom value is the maximum requested encoder quality. If that would exceed the source byte size, Image Bench automatically selects a lower fitting quality so JPEG output never grows. PNG remains lossless; Image Bench maps the same control to lossless compression effort while preserving the existing preset anchor points.

For JPEG, the percentage is a quality ceiling rather than a claimed compression percentage. For PNG, Image Bench remains lossless and maps the same presets to encoder compression effort.

## Output safety

- Output files are created with exclusive-create semantics.
- Filename suffixes are added before the extension; the default is `-optimized`.
- The optional quality suffix adds the effective quality percentage. For JPEG this reflects any automatic quality reduction required by the no-larger rule; for example a selected 85% preset may become `-optimized-77` when 77 is the highest fitting quality.
- Existing files are never replaced; a collision receives a final `-2`, `-3`, etc. counter.
- Source files are never modified.
- The Flatpak manifest deliberately avoids broad host-filesystem access; the user selects input/output locations explicitly.

## Metadata and color note

Image Bench re-encodes every output image. v0.9.0 does **not** promise preservation of EXIF/XMP metadata or embedded ICC profiles. Metadata/profile behavior therefore remains part of owner runtime acceptance before this candidate is called stable.

## Rust migration status

The Rust tree is being migrated in controlled parity gates. The current migration candidate contains the Rust domain layer, GNOME application/UI shell, import/DnD queue state, queue thumbnails, output-folder selection, the native Glycin processing/export path, and a real in-memory Original/Compare/Preview inspection mode. Localization is now integrated for EN/NL/DE/FR/ES/IT/PT. Remaining release/runtime gates are still pending, so this candidate is **not** yet a parity-complete release.

## Build

Development build:

```bash
meson setup build -Dprofile=development
meson compile -C build
```

Cargo validation:

```bash
cargo fmt --check
cargo check
cargo clippy -- -D warnings
cargo test
```

The Flatpak development manifest uses the Rust SDK extension. For this first compile gate only, Cargo is allowed build-time network access to resolve crates; the release manifest must use vendored/offline Cargo sources.

## Flatpak

```bash
flatpak-builder --user --install --force-clean build-dir io.github.christiaanbruinsma.ImageBench.Devel.yml
flatpak run io.github.christiaanbruinsma.ImageBench.Devel
```

## Runtime acceptance

Before promoting this candidate to stable, verify on the target GNOME/Flatpak runtime:

1. App launches and adaptive sidebar/content layout behaves correctly.
2. Multi-file JPEG/PNG import works.
3. Folder import works for a larger batch.
4. 1920 / 1280 / 800 / Custom produce the expected proportional dimensions.
5. Images smaller than the target width are not upscaled.
6. High / Balanced / Compact / Custom produce valid output for JPEG and PNG.
7. Existing source/output files are never overwritten.
8. A selected output folder remains writable through the Flatpak file-chooser grant.
9. Glycin decode/encode works inside the installed Flatpak sandbox.
10. Visual color/output quality is acceptable on representative client photos.

## License

GPL-3.0-or-later.

Rust Candidate 10 uses a native right-side batch queue with native row separators sidebar via `Adw.OverlaySplitView`; it collapses to an overlay on narrower windows.

The workspace uses separate native libadwaita panes: settings on the left, preview in the center, and a right Batch queue sidebar with its own header.
- The batch queue uses a native right sidebar with a flat navigation-style list and no nested boxed surface.
