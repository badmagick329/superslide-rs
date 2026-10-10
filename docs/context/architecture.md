# Architecture

Target design for v1. The slices exist as scaffolds holding their core types; as each slice's issue lands, rewrite its section to describe the code as built, and point at the code rather than restating it. Bindings live in [keymap.md](keymap.md); terms in [glossary.md](glossary.md).

## Shape

One binary crate, `superslide-rs` ([Cargo.toml](../../Cargo.toml)), producing `superslide.exe`. Release builds set `#![windows_subsystem = "windows"]` ([main.rs](../../src/main.rs)) so no console opens.

The code is vertical slices, one folder per feature, each split into the layers it uses: `domain` (types and rules, no I/O crates), `infrastructure` (filesystem, threads, third-party decoders) and `presentation` (egui). Dependencies point inward: `shell` uses slices, a slice's `infrastructure` and `presentation` use its `domain`, and a slice reaches another slice only through that slice's `domain`. No slice imports `shell`. Why, and when a layer is added: [the vertical-slices decision](../decisions/2026-10-10-vertical-slices.md).

| Slice | Layers | Owns |
| --- | --- | --- |
| [`shell`](../../src/shell/mod.rs) | | The outer layer: [`app`](../../src/shell/app.rs) (the winit `ApplicationHandler`: owns all state, runs commands), [`command`](../../src/shell/command.rs) (`Command`, everything the owner can ask for), [`keymap`](../../src/shell/keymap.rs) (the only place bindings are decided) and [`render`](../../src/shell/render.rs) (wgpu, textures, mips, the quad pipeline, egui painting). |
| [`image_file`](../../src/image_file/mod.rs) | domain | The shared kernel: `Format` (the one list of extensions, and their signatures) and EXIF `Orientation`. |
| [`browse`](../../src/browse/mod.rs) | domain, infrastructure | The image list and its order in `domain`; scanning paths into it, on its own thread, with the folder filter, in `infrastructure`. |
| [`load`](../../src/load/mod.rs) | domain, infrastructure | `Decoded`, `DecodeError` and the trust rules (content matches name, size cap, fit to GPU) in `domain`; the decoders in `infrastructure`. The preload cache and its workers land here (SSR-11). |
| [`view`](../../src/view/mod.rs) | domain | The view transform (fit, zoom, pan, screen rotation, EXIF orientation) and the magnification filter. Pure math. |
| [`notices`](../../src/notices/mod.rs) | domain, presentation | Notice timing and stacking in `domain`; egui drawing and count wording in `presentation`. |
| [`prefs`](../../src/prefs/mod.rs) | domain | `Preferences` and their defaults. It holds other slices' domain types, such as the magnification filter. |
| [`info_overlay`](../../src/info_overlay/mod.rs), [`file_actions`](../../src/file_actions/mod.rs), [`clipboard`](../../src/clipboard/mod.rs) | domain, domain, none | Scaffolds for SSR-17, SSR-15 and SSR-16. |

[`main`](../../src/main.rs) reads command-line paths, builds the event loop and hands control to `shell::app`. Unit tests sit beside the code they test; every `domain` and the `browse` and `load` infrastructure carry them, and none needs a window or GPU, so tests run headless.

## Threads

- **Main thread**: the winit event loop, all state, all wgpu calls, egui. Nothing on it blocks on decoding or scanning; the only blocking calls are the native file and folder dialogs, which are modal by nature.
- **Decode workers**: `min(available_parallelism - 1, 4)` threads, at least 1, taking jobs from the preload cache's priority queue.
- **Scan thread**: one per open, walking the filesystem; sends the finished, sorted image list back, plus progress counts for a notice. Dropping the `browse::infrastructure::Scan` handle cancels it, and the app tags each scan's events with a generation number, so a newer open replaces an older one and its late events are ignored.

Workers and the scan thread send `shell::app::AppEvent`s through the event loop's `EventLoopProxy`, which also wakes the loop. GPU upload happens on the main thread when the result arrives.

## Image change

Navigation moves the current image immediately in the image list but keeps drawing the previous texture until the new one is ready, then swaps in a single frame: no blank frame, ever. After 150 ms without the new image, a notice shows `Loading…`. A file that fails to decode gets an error card naming the file and reason; navigation continues past it.

Navigation wraps from the last image to the first and back, with a notice saying so.

## Preload cache

- Keyed by path, so re-sorting keeps decoded images.
- The **preload window** is the current image, then 3 ahead and 1 behind in the direction of travel (reversed when the owner navigates backwards). Job priority follows that order; the current image always outranks the rest.
- Budget: GPU texture bytes including mips (`w × h × 4 × 4/3`), default 1024 MB. Over budget, evict the entry furthest from the current image outside the preload window.
- A worker drops a job that left the preload window before it started. A decode already running finishes and its result is cached if it still fits; mid-decode cancellation is not attempted.
- CPU pixels are freed after upload. Ctrl+C re-decodes the file.

## Decoding

Built in the `load` slice: [infrastructure.rs](../../src/load/infrastructure.rs)'s `decode(path, max_dim)` returns `Decoded` (RGBA8 pixels, their size, the original size for the info overlay, EXIF orientation) or a `DecodeError`, both from [domain.rs](../../src/load/domain.rs), which also holds the checks below that need no decoder. Not yet called by the app; the preload cache's workers will call it.

- Formats by extension, case-insensitive: `jpg`, `jpeg`, `png`, `tga`, `bmp`. `image_file::domain::Format::from_path` is the one list; the scan's `browse::infrastructure::is_image` asks it. JPEG uses `zune-jpeg`; the rest use `image`.
- The leading bytes must confirm the extension. Another known format's signature gives `Misnamed: a PNG file, not JPEG`; TGA has no signature, so only another format's signature rejects a `.tga`. Browsers do save misnamed files: one of 101 in the owner's Downloads was a PNG named `.jpg`.
- `DecodeError` is `Io`, `Unsupported` (misnamed, too large) or `Corrupt` (empty, wrong content, damaged, cut short); its `Display` is the error card's one line.
- Images over 2^28 pixels (1 GiB as RGBA) are refused from the header, before any pixel buffer exists. zune-jpeg's own 16384 px per side cap is lifted; `image`'s 512 MiB allocation cap is raised to match.
- A cut-short JPEG is an error, not a grey-filled image. zune-jpeg's lenient mode reports success on those and its strict mode also rejects stray bytes some cameras write, so decoding stays lenient and `scan_is_complete` checks for an EOI after the first top-level SOS, skipping segments by length because an EXIF thumbnail carries its own SOS and EOI.
- EXIF orientation is read (JPEG only, `kamadak-exif`) and returned beside the pixels; missing or invalid tags mean `Normal`. The display path never rotates pixels on the CPU (only Ctrl+C does, see *Clipboard*).
- An image whose width or height exceeds `max_dim` (the device's `max_texture_dimension_2d`, read once at startup) is downscaled, keeping aspect, with `fast_image_resize` (Lanczos3, alpha premultiplied); the long side lands on `max_dim` exactly.
- Measured on the owner's PC, release build: an 18.7 MB, 8660 × 5773 (50 MP) JPEG in 400 ms.
- The orientation fixtures in `tests/fixtures/` are 8 × 4 JPEGs, left half red, written by Windows GDI+, so the EXIF they carry comes from a writer independent of the decoder.

## Rendering

- Built in [render.rs](../../src/shell/render.rs) and [quad.wgsl](../../src/shell/quad.wgsl): `wgpu` on the Dx12 backend, an sRGB surface format, `PresentMode::Fifo` (vsync, no tearing). If no hardware adapter is found, request one with `force_fallback_adapter` (WARP); `SUPERSLIDE_FORCE_WARP=1` forces that path for testing. The chosen adapter is logged to stderr, which only debug builds show.
- The device is requested with the adapter's own limits, not wgpu's defaults: the defaults cap textures at 8192 px where Dx12 hardware takes 16384. `Renderer::max_texture_dimension` holds the result for decode.
- A startup failure (no window, no adapter, no device) shows a message box and exits: no overlay exists yet and release builds have no console.
- One texture per cached image (`shell::render::ImageTexture`), `Rgba8UnormSrgb`, full mip chain (`floor(log2(max(w, h))) + 1` levels), generated after upload by a render pass per level ([mip.wgsl](../../src/shell/mip.wgsl)) that samples the level above with a linear filter. The sRGB format makes that average gamma-correct: 1 px black-and-white stripes shrink to sRGB 188, not 128. `ImageTexture::bytes` reports `w × h × 4 × 4/3` for the preload budget.
- `Renderer::upload` writes pixels in slices of about 16 MB, since `write_texture` stages each call in one buffer and wgpu's default buffer limit is 256 MB. It blocks until the GPU finishes, so the stderr log of each upload's time is real and staging memory is freed before the next upload. Measured on an RTX 3070, release build: 48 MP in 53–75 ms, warm.
- Until decoding lands, a generated image stands in: a checkerboard, or with `SUPERSLIDE_STAND_IN=stripes` an 8000 × 6000 image of 1 px stripes for checking moiré and upload time; `white` and `black` are solid, for checking overlay legibility.
- One quad, one draw, no vertex buffer. A uniform holds a 2×2 matrix and offset from `view` (`view::domain::Transform`, which documents the coordinates) plus the window size, mapping the quad to window pixels. EXIF orientation (including its mirrorings) and screen rotation are both folded into that matrix.
- Sampling: trilinear by default. Ctrl+F switches magnification to nearest, for sharp pixels at high zoom; minification stays mipmapped linear so a shrunk image never aliases. The quad's bind group 0 holds the uniforms and sampler, one prebuilt group per filter, so the toggle rebuilds nothing; group 1 is the image.
- egui (`egui-winit` for input, `egui-wgpu` for painting) draws on the same device after the image, in the same render pass. egui's texture changes are applied even on a skipped frame, or a font atlas sent then would never arrive.

## Overlays and notices

- Notices live in the `notices` slice: `Notices` in [domain.rs](../../src/notices/domain.rs) decides what shows and how opaque, and [presentation.rs](../../src/notices/presentation.rs) draws it: top-right, oldest on top, at most 3 (a fourth drops the oldest). Each is fully visible for 3 s, then fades out over 0.5 s. Repeating a shown text restarts its timer and moves it to the bottom instead of stacking; `Notices::notify_as` gives a changing text, such as scan progress, a key so each update replaces the last. White 20 pt text on black at 240/255 alpha: the surface is sRGB, so egui blends in linear light, and lower alphas leave a mid-grey backing on white images. The top-left corner is left to the info overlay.
- Redraws are on demand. `App::about_to_wait` sleeps until a notice starts fading or egui asks for a frame (it does after an area's first frame, which it spends measuring with nothing visible), and redraws every frame while a notice fades. Nothing animates once notices are gone: idle CPU is zero.
- Input routing: every window event goes to egui first, so its pointer and modifier state stay current, but egui keeps an event only while one of its widgets has focus (the preferences screen). Otherwise all input goes to `keymap`. Notices never take clicks, so a drag that starts on a notice still pans.

## View transform

Built in [view/domain.rs](../../src/view/domain.rs) as `View`: one per shown image, holding the stored image size, window size, EXIF orientation, quarter-turns of screen rotation, zoom relative to fit, and the image point at the window centre. That point is kept in image coordinates, so a window resize keeps the same detail centred at the same zoom relative to fit. `View::transform` produces the quad uniform's matrix and offset; orientation and screen rotation compose into one signed-permutation matrix, so mirrored orientations need no special case. Opening an image starts at fit and centred; screen rotation survives navigation away and back, per path, until the app closes (`View::turns` carries it; wired with navigation).

- Zoom from right-drag or the wheel anchors at the cursor; from keys, at the window centre. Zoom range 1/8× fit to 32× native pixels, except that fit stays reachable when it is already past 32× (a tiny image in a large window).
- Pan clamps so the image cannot leave the window: an image smaller than the window on an axis stays centred on that axis; a larger one keeps covering the window. Every zoom, pan and resize reapplies the clamp.
- R / L return to fit: after a quarter turn the old zoom and pan would land somewhere arbitrary.
- Arrow-key pan moves 10% of the window per press and scrolls the view: Right shows more of the right side.

## Mouse

Built in [keymap.rs](../../src/shell/keymap.rs) as `keymap::Mouse`. A press that moves at most 4 px from where it went down is a click on release; beyond that it is a drag, and the first drag step includes the slop so the image stays under the cursor. Left click previous, right click next; left drag pans, right drag zooms about where the drag started (200 px up doubles). One button at a time: a second button pressed during a gesture is ignored. The wheel zooms 1.25× per notch about the cursor; touchpad pixel scrolling counts 50 px as a notch.

## Advance limit and slideshow

One timer drives both, through `ControlFlow::WaitUntil`.

- The first press of Space or Backspace always advances. Key-repeat events advance only once the advance limit has passed since the last advance.
- The slideshow advances every slideshow interval, but only once the next image is in the preload cache; a manual navigation restarts the interval.

## Opening

Built in [browse/infrastructure.rs](../../src/browse/infrastructure.rs): `Open` names the route, `scan` walks it (`walkdir`) and returns a `Found` holding the `ImageList`, the count of unreadable folders (skipped, never fatal) and the paths given to open that do not exist. Only command-line paths are wired so far; they open through the same `Open::Paths` route drops will use. Until navigation lands, the result is only a notice (`12,345 images`, plus `Skipped 3 unreadable folders` or `Not found: …` when they apply).

| Route | Image list |
| --- | --- |
| A file (command line, drop, O) | Images in its folder, not recursive; current image is that file; sort by name. |
| A folder (command line, drop) | Images in it, not recursive; sort by name. |
| Shift+O | Recursive open of the chosen folder; shuffle. |
| Several paths (command line, or drops in one burst) | Each file itself and each folder's images, not recursive, deduplicated; current image is the first image file named; sort by name. |

- The folder filter applies to every folder below the opened root, matching folder names case-insensitively; the root itself is never filtered.
- Progress goes out at most every 100 ms, so a scan that finishes sooner shows no `Scanning…` notice. Measured: 100,000 files scanned in 0.3 s warm (release), 1.4 s in a debug build while zooming, with the window redrawing throughout.

## Image list order

[browse/domain.rs](../../src/browse/domain.rs): `ImageList` holds the entries in display order and the current image; `Order` is the sort mode plus reverse and grouping. `ImageList::set_order` re-sorts in memory and keeps the current image.

- Name sort is natural and case-insensitive (`img2` before `img10`), on the file name only; date is the file's modified time; size is bytes. Ties fall back to name, then the full path, so every order is total.
- Shuffle starts from path order and runs Fisher–Yates on SplitMix64 from the stored `u64` seed, so a seed rebuilds the same order whatever the scan order; a test pins the output, since changing the generator would scramble restored sessions.
- Reverse turns the order within each group round; grouping keeps groups in natural folder-path order either way. A grouped shuffle shuffles inside each group.
- Each entry caches its lowercased file name and folder at scan time. Without that, re-sorting 100,000 images took 330 ms; with it, about 55 ms (release), on the main thread.

## Files and persistence

- `superslide-prefs.toml` and `superslide-session.toml`, both beside `superslide.exe`. Writes go to a temp file, then rename.
- Unwritable or corrupt file: run on defaults, show one notice, keep running. A corrupt file is renamed `.bak` before being replaced.
- Session is saved on exit and at most every 10 s while it has changed.
- Move and copy never overwrite: a name collision skips the file with a notice.
- After delete or move, the image leaves the image list and the cache, and the next image becomes current.

## Clipboard

Ctrl+V shows the clipboard image in place of the current image without adding it to the image list; file actions and Ctrl+C are disabled on it, and the next navigation returns to the image list where it left off. Ctrl+C on a file re-decodes it, applies EXIF orientation and screen rotation on the CPU so the copy matches the screen, and puts the RGBA pixels on the clipboard.

## Preference defaults

| Preference | Default |
| --- | --- |
| Advance limit | 500 ms |
| Slideshow interval | 10 s |
| Folder filter | `private` |
| Preload budget | 1024 MB |
| Start fullscreen | off |
| Session restore | on |
| Magnification filter | linear |

## Build

- `[profile.dev.package."*"] opt-level = 3`: unoptimised decoders take seconds per 50 MP JPEG and make dev builds unusable.
- Release: `lto = "thin"`, `codegen-units = 1`.
- Toolchain: stable, edition 2024.
- `wgpu` and `egui-wgpu` build without default features, so only the Dx12 backend compiles; `image` builds only PNG, TGA and BMP, since JPEG goes through `zune-jpeg`; tests add its `jpeg` feature as a dev-dependency to encode JPEGs, which the shipped build does not compile. The defaults would also compile Vulkan, GL, Metal, WebGL, AVIF and WebP support the viewer never uses.
- A scaffolded module carries `#![expect(dead_code, reason = "scaffold: …")]`. Once the module is wired up, the expectation goes unfulfilled and `clippy -D warnings` fails: delete the attribute then.
