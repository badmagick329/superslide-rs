# Architecture

Target design for v1. The modules exist as scaffolds holding their core types; as each module's issue lands, rewrite its section to describe the code as built, and point at the code rather than restating it. Bindings live in [keymap.md](keymap.md); terms in [glossary.md](glossary.md).

## Shape

One binary crate, `superslide-rs` ([Cargo.toml](../../Cargo.toml)), producing `superslide.exe`. Release builds set `#![windows_subsystem = "windows"]` ([main.rs](../../src/main.rs)) so no console opens.

| Module | Owns |
| --- | --- |
| [`main`](../../src/main.rs) | Command-line paths, building the event loop, handing control to `app`. |
| [`app`](../../src/app.rs) | The winit `ApplicationHandler`: turns input into `Command`s, owns all state, runs commands. |
| [`keymap`](../../src/keymap.rs) | Maps keys and mouse gestures to `Command`. The only place bindings are decided. |
| [`library`](../../src/library.rs) | Scanning paths into the image list, the folder filter, sort modes, shuffle, grouping. No GPU, no I/O beyond the filesystem scan. |
| [`decode`](../../src/decode.rs) | Turning a path into oriented-at-draw RGBA pixels: format dispatch, EXIF orientation read, oversize downscale. |
| [`cache`](../../src/cache.rs) | The preload cache and its decode worker pool. |
| [`view`](../../src/view.rs) | The view transform: fit, zoom, pan, screen rotation, EXIF orientation. Pure math. |
| [`render`](../../src/render.rs) | wgpu device and surface, image texture upload, mip generation, the quad pipeline, egui painting. |
| [`ui`](../../src/ui.rs) | egui content: info overlay, notices, preferences screen. |
| [`files`](../../src/files.rs) | Delete to Recycle Bin, move, copy. |
| [`clipboard`](../../src/clipboard.rs) | Ctrl+C and Ctrl+V through `arboard`. |
| [`prefs`](../../src/prefs.rs) | `Preferences` and `Session`: defaults, TOML load and atomic save beside the executable. |

`library`, `view`, `decode`, `cache` (policy, not threads) and `prefs` carry the unit tests; they take no window or GPU so tests run headless.

## Threads

- **Main thread**: the winit event loop, all state, all wgpu calls, egui. Nothing on it blocks on decoding or scanning; the only blocking calls are the native file and folder dialogs, which are modal by nature.
- **Decode workers**: `min(available_parallelism - 1, 4)` threads, at least 1, taking jobs from the preload cache's priority queue.
- **Scan thread**: one per open, walking the filesystem; sends the finished image list back, plus progress counts for a notice.

Workers and the scan thread report through a channel and wake the loop with `EventLoopProxy::send_event`. GPU upload happens on the main thread when the result arrives.

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

- Formats by extension, case-insensitive: `jpg`, `jpeg`, `png`, `tga`, `bmp`. JPEG uses `zune-jpeg`; the rest use `image`.
- EXIF orientation is read (JPEG only) and returned beside the pixels; the display path never rotates pixels on the CPU (only Ctrl+C does, see *Clipboard*).
- An image whose width or height exceeds the device's `max_texture_dimension_2d` is downscaled on the worker, keeping aspect, with `fast_image_resize`. The limit is read once at startup and passed to workers.

## Rendering

- Built in [render.rs](../../src/render.rs) and [quad.wgsl](../../src/quad.wgsl): `wgpu` on the Dx12 backend, an sRGB surface format, `PresentMode::Fifo` (vsync, no tearing). If no hardware adapter is found, request one with `force_fallback_adapter` (WARP); `SUPERSLIDE_FORCE_WARP=1` forces that path for testing. The chosen adapter is logged to stderr, which only debug builds show.
- The device is requested with the adapter's own limits, not wgpu's defaults: the defaults cap textures at 8192 px where Dx12 hardware takes 16384. `Renderer::max_texture_dimension` holds the result for decode.
- A startup failure (no window, no adapter, no device) shows a message box and exits: no overlay exists yet and release builds have no console.
- One texture per cached image, `Rgba8UnormSrgb`, full mip chain (`floor(log2(max(w, h))) + 1` levels), generated after upload by a render pass per level that samples the level above with a linear filter.
- One quad, one draw, no vertex buffer. A uniform holds a 2×2 matrix and offset from `view` (`view::Transform`, which documents the coordinates) plus the window size, mapping the quad to window pixels. EXIF orientation (including its mirrorings) and screen rotation are both folded into that matrix.
- Sampling: trilinear by default. Ctrl+F switches magnification to nearest, for sharp pixels at high zoom; minification stays mipmapped linear so a shrunk image never aliases.
- egui draws on the same device after the image, in the same frame.

## View transform

Built in [view.rs](../../src/view.rs) as `View`: one per shown image, holding the stored image size, window size, EXIF orientation, quarter-turns of screen rotation, zoom relative to fit, and the image point at the window centre. That point is kept in image coordinates, so a window resize keeps the same detail centred at the same zoom relative to fit. `View::transform` produces the quad uniform's matrix and offset; orientation and screen rotation compose into one signed-permutation matrix, so mirrored orientations need no special case. Opening an image starts at fit and centred (until textures land, a generated checkerboard stands in for the image); screen rotation survives navigation away and back, per path, until the app closes (`View::turns` carries it; wired with navigation).

- Zoom from right-drag or the wheel anchors at the cursor; from keys, at the window centre. Zoom range 1/8× fit to 32× native pixels, except that fit stays reachable when it is already past 32× (a tiny image in a large window).
- Pan clamps so the image cannot leave the window: an image smaller than the window on an axis stays centred on that axis; a larger one keeps covering the window. Every zoom, pan and resize reapplies the clamp.
- R / L return to fit: after a quarter turn the old zoom and pan would land somewhere arbitrary.
- Arrow-key pan moves 10% of the window per press and scrolls the view: Right shows more of the right side.

## Mouse

Built in [keymap.rs](../../src/keymap.rs) as `keymap::Mouse`. A press that moves at most 4 px from where it went down is a click on release; beyond that it is a drag, and the first drag step includes the slop so the image stays under the cursor. Left click previous, right click next; left drag pans, right drag zooms about where the drag started (200 px up doubles). One button at a time: a second button pressed during a gesture is ignored. The wheel zooms 1.25× per notch about the cursor; touchpad pixel scrolling counts 50 px as a notch.

## Advance limit and slideshow

One timer drives both, through `ControlFlow::WaitUntil`.

- The first press of Space or Backspace always advances. Key-repeat events advance only once the advance limit has passed since the last advance.
- The slideshow advances every slideshow interval, but only once the next image is in the preload cache; a manual navigation restarts the interval.

## Opening

| Route | Image list |
| --- | --- |
| A file (command line, drop, O) | Images in its folder, not recursive; current image is that file; sort by name. |
| A folder (command line, drop) | Images in it, not recursive; sort by name. |
| Shift+O | Recursive open of the chosen folder; shuffle. |
| Several drops in one burst | The union, deduplicated; sort by name. |

The folder filter applies to every folder below the opened root, matching folder names case-insensitively. Name sort is natural and case-insensitive (`img2` before `img10`). Date is the file's modified time. Shuffle stores its seed so session restore can rebuild the same order.

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
- `wgpu` and `egui-wgpu` build without default features, so only the Dx12 backend compiles; `image` builds only PNG, TGA and BMP, since JPEG goes through `zune-jpeg`. The defaults would also compile Vulkan, GL, Metal, WebGL, AVIF and WebP support the viewer never uses.
- A scaffolded module carries `#![expect(dead_code, reason = "scaffold: …")]`. Once the module is wired up, the expectation goes unfulfilled and `clippy -D warnings` fails: delete the attribute then.
