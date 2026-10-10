# Architecture

The v1 design. Every section up to *Image list order* describes phase 1 as built and verified (SSR-14), linking the code, and *File actions* describes SSR-15 as built. *Files and persistence* and *Clipboard* are still the target for phases 2 and 3, and *Preference defaults* lists defaults the code already uses but no file sets yet; as each of those issues lands, rewrite its section to describe the code, pointing at it rather than restating it. Bindings live in [keymap.md](keymap.md); terms in [glossary.md](glossary.md).

## Shape

One binary crate, `superslide-rs` ([Cargo.toml](../../Cargo.toml)), producing `superslide.exe`. Release builds set `#![windows_subsystem = "windows"]` ([main.rs](../../src/main.rs)) so no console opens.

The code is vertical slices, one folder per feature, each split into the layers it uses: `domain` (types and rules, no I/O crates), `infrastructure` (filesystem, threads, third-party decoders) and `presentation` (egui). Dependencies point inward: `shell` uses slices, a slice's `infrastructure` and `presentation` use its `domain`, and a slice reaches another slice only through that slice's `domain`. No slice imports `shell`. Why, and when a layer is added: [the vertical-slices decision](../decisions/2026-10-10-vertical-slices.md).

| Slice | Layers | Owns |
| --- | --- | --- |
| [`shell`](../../src/shell/mod.rs) | | The outer layer: [`app`](../../src/shell/app.rs) (the winit `ApplicationHandler`: owns all state, runs commands), [`command`](../../src/shell/command.rs) (`Command`, everything the owner can ask for), [`keymap`](../../src/shell/keymap.rs) (the only place bindings are decided) and [`render`](../../src/shell/render.rs) (wgpu, textures, mips, the quad pipeline, egui painting). |
| [`image_file`](../../src/image_file/mod.rs) | domain | The shared kernel: `Format` (the one list of extensions, and their signatures) and EXIF `Orientation`. |
| [`browse`](../../src/browse/mod.rs) | domain, infrastructure, presentation | The image list, its order and group jumps in `domain`, with the advance limit in [`domain::advance`](../../src/browse/domain/advance.rs) and the slideshow timer in [`domain::slideshow`](../../src/browse/domain/slideshow.rs); scanning paths into it, on its own thread, with the folder filter, in `infrastructure`; the wording of the order notice in `presentation`. |
| [`load`](../../src/load/mod.rs) | domain, infrastructure, presentation | `Decoded`, `DecodeError` and the trust rules (content matches name, size cap, fit to GPU) in `domain`, with the preload cache's policy in [`domain::preload`](../../src/load/domain/preload.rs); the decoders and the decode workers ([`infrastructure::workers`](../../src/load/infrastructure/workers.rs)) in `infrastructure`; the error card in `presentation`. |
| [`view`](../../src/view/mod.rs) | domain | The view transform (fit, zoom, pan, screen rotation, EXIF orientation) and the magnification filter. Pure math. |
| [`notices`](../../src/notices/mod.rs) | domain, presentation | Notice timing and stacking in `domain`; egui drawing and count wording in `presentation`. |
| [`prefs`](../../src/prefs/mod.rs) | domain | `Preferences` and their defaults. It holds other slices' domain types, such as the magnification filter. |
| [`file_actions`](../../src/file_actions/mod.rs) | domain, infrastructure, presentation | Delete, move and copy: `Action` and `Failure` in `domain`; the file action thread and the no-overwrite filesystem calls in `infrastructure`; the notice wording in `presentation`. |
| [`info_overlay`](../../src/info_overlay/mod.rs), [`clipboard`](../../src/clipboard/mod.rs) | domain, none | Scaffolds for SSR-17 and SSR-16. |

A layer that grows past one concern becomes a file with submodules beside it (`load/domain.rs` and `load/domain/preload.rs`), so paths still name the layer.

[`main`](../../src/main.rs) reads command-line paths, builds the event loop and hands control to `shell::app`. Unit tests sit beside the code they test; every `domain` and the `browse`, `load` and `file_actions` infrastructure carry them, and none needs a window or GPU, so tests run headless.

## Threads

- **Main thread**: the winit event loop, all state, all wgpu calls, egui. Nothing on it blocks on decoding, scanning or file actions; the only blocking calls are the native file and folder dialogs, which are modal by nature.
- **Decode workers**: `min(available_parallelism - 1, 4)` threads, at least 1, taking the highest-priority job from one shared queue (`Workers` in [workers.rs](../../src/load/infrastructure/workers.rs)). A decoder panic is caught and becomes that file's error, so a hostile file costs one image, not a worker. Dropping `Workers` stops them after their current decode.
- **Scan thread**: one per open, walking the filesystem; sends the finished, sorted image list back, plus progress counts for a notice. Dropping the `Scan` handle ([browse/infrastructure.rs](../../src/browse/infrastructure.rs)) cancels it, and the app tags each scan's events with a generation number, so a newer open replaces an older one and its late events are ignored.
- **File action thread**: one, running delete, move and copy in the order asked (`FileActions` in [file_actions/infrastructure.rs](../../src/file_actions/infrastructure.rs)). A Recycle Bin call or a copy to a slow drive can take seconds. Dropping `FileActions` waits for the queue to finish, so closing the window mid-copy never leaves half a file.

Workers, the scan thread and the file action thread send `AppEvent`s ([app.rs](../../src/shell/app.rs)) through the event loop's `EventLoopProxy`, which also wakes the loop. GPU upload happens on the main thread when the result arrives, and the CPU pixels are dropped right after.

## Image change

Built in [app.rs](../../src/shell/app.rs) (`Shown::show_current`). Navigation moves the current image in the image list at once, but the screen keeps the previous texture (`Showing`) until the new image is in the preload cache, then swaps in a single frame: no blank frame, ever. Each image opens at fit. After 150 ms without the new image, a `Loading…` notice shows; it is dismissed the moment the image arrives. A file that fails to decode shows an error card, centred, with its file name and the `DecodeError` line; view commands do nothing on it and navigation continues past it.

Next and previous wrap at the ends (`ImageList::step`) with a notice, `Wrapped to the first image` or `Wrapped to the last image`, since a wrap is easy to miss; Ctrl+Left/Right group jumps wrap the same way. The window title is `<file name> — SuperSlide-RS` for what is on screen, error cards included, so it never names an image still loading.

## Preload cache

The policy is [preload.rs](../../src/load/domain/preload.rs), generic over what it holds so its tests need no GPU; `shell::app` stores textures in it and drives the workers.

- `preload_window` gives the current image, then 3 ahead and 1 behind in the direction of travel (the last navigation's direction), wrapping at the list's ends. That order is the job priority.
- On every navigation or open, the cache is retargeted and `Workers::request` replaces the whole queue with the window's images not yet held, so a job that left the window before a worker reached it is never decoded. A decode already running finishes; an image requested again while it runs is not queued twice.
- Keyed by path, so re-sorting keeps decoded images. Each entry also keeps its image list position for distances; `PreloadCache::reindex` refreshes them after the list changes and drops entries no longer in it.
- Budget: GPU texture bytes including mips (`render::texture_bytes`, `w × h × 4 × 4/3`), default 1024 MB. Over budget, the entry furthest from the current image outside the preload window goes, distance measured round the wrap.
- The preload window itself is never evicted, even past the budget: an image the owner is about to see must not be thrown away and decoded again. With images above about 200 MB of texture (about 40 MP), the cache can exceed the budget by up to the window's five images.
- A result outside the window is kept only if it fits as things stand, and is checked before upload, so a result nobody keeps is never uploaded. Failed decodes are cached too, at no cost, so returning to one shows its error card without decoding it again.
- The image on screen is shared (`Rc`) between the cache and the screen, so evicting it does not free it mid-display.
- Debug builds log the cache's size to stderr after each result.
- Measured on an RTX 3070, debug build, 200 JPEGs of 6000 × 4000 (122 MB of texture each): paging forward and backward at 2 presses a second for a minute, every image was on screen within 50 ms of the key, except one cold image just after starting backwards. The cache held 976 MB throughout; Task Manager's dedicated GPU memory stayed flat at about 1.5 GB, which is 274 MB for the app with no image, the cache, and wgpu allocator and staging slack.
- Measured on the same PC, release build, the SSR-14 set (30 JPEGs of 8660 × 5773, 267 MB of texture each, so the preload window alone is past the budget): 60 presses of Space about 0.75 s apart, two laps with wraps, every image on screen within 50 ms of the key; dedicated GPU memory flat at 1.67 GB.
- Ctrl+C (SSR-16) will re-decode the file rather than use the cache; see *Clipboard*.

## Decoding

Built in the `load` slice: [infrastructure.rs](../../src/load/infrastructure.rs)'s `decode(path, max_dim)` returns `Decoded` (RGBA8 pixels, their size, the original size for the info overlay, EXIF orientation) or a `DecodeError`, both from [domain.rs](../../src/load/domain.rs), which also holds the checks below that need no decoder. The decode workers call it.

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
- One texture per cached image (`shell::render::ImageTexture`), `Rgba8UnormSrgb`, full mip chain (`floor(log2(max(w, h))) + 1` levels), generated after upload by a render pass per level ([mip.wgsl](../../src/shell/mip.wgsl)) that samples the level above with a linear filter. The sRGB format makes that average gamma-correct: 1 px black-and-white stripes shrink to sRGB 188, not 128. `render::texture_bytes` gives `w × h × 4 × 4/3` for the preload budget, from the size alone, so a result can be turned down before upload.
- `Renderer::upload` writes pixels in slices of about 16 MB, since `write_texture` stages each call in one buffer and wgpu's default buffer limit is 256 MB. It blocks until the GPU finishes, so the stderr log of each upload's time is real and staging memory is freed before the next upload. Measured on an RTX 3070, release build: 48 MP in 53–75 ms, warm.
- Until the first image opens, a generated image stands in: a checkerboard, or with `SUPERSLIDE_STAND_IN=stripes` an 8000 × 6000 image of 1 px stripes for checking moiré and upload time; `white` and `black` are solid, for checking overlay legibility.
- One quad, one draw, no vertex buffer; on an error card, no draw, just the black clear under egui. A uniform holds a 2×2 matrix and offset from `view` (`view::domain::Transform`, which documents the coordinates) plus the window size, mapping the quad to window pixels. EXIF orientation (including its mirrorings) and screen rotation are both folded into that matrix.
- Measured (release, `stripes.png` from the SSR-14 set, panned by drag and arrows in a window smaller than the image): at 0.25× native every pixel reads 188, so no moiré or shimmer. Between mip levels the 1 px stripes are a worst case trilinear does not fully hide: at 0.6× a fine beat pattern of 164–209 remains.
- Sampling: trilinear by default. Ctrl+F switches magnification to nearest, for sharp pixels at high zoom; minification stays mipmapped linear so a shrunk image never aliases. The quad's bind group 0 holds the uniforms and sampler, one prebuilt group per filter, so the toggle rebuilds nothing; group 1 is the image.
- egui (`egui-winit` for input, `egui-wgpu` for painting) draws on the same device after the image, in the same render pass. egui's texture changes are applied even on a skipped frame, or a font atlas sent then would never arrive.

## Overlays and notices

- Notices live in the `notices` slice: `Notices` in [domain.rs](../../src/notices/domain.rs) decides what shows and how opaque, and [presentation.rs](../../src/notices/presentation.rs) draws it: top-right, oldest on top, at most 3 (a fourth drops the oldest). Each is fully visible for 3 s, then fades out over 0.5 s. Repeating a shown text restarts its timer and moves it to the bottom instead of stacking; `Notices::notify_as` gives a changing text, such as scan progress, a key so each update replaces the last. White 20 pt text on black at 240/255 alpha: the surface is sRGB, so egui blends in linear light, and lower alphas leave a mid-grey backing on white images. The top-left corner is left to the info overlay.
- Redraws are on demand. `App::about_to_wait` sleeps until a notice starts fading or egui asks for a frame (it does after an area's first frame, which it spends measuring with nothing visible), and redraws every frame while a notice fades. Nothing animates once notices are gone: idle CPU is zero.
- Input routing: every window event goes to egui first, so its pointer and modifier state stay current, but egui keeps an event only while one of its widgets has focus (the preferences screen). Otherwise all input goes to `keymap`. Notices never take clicks, so a drag that starts on a notice still pans.

## View transform

Built in [view/domain.rs](../../src/view/domain.rs) as `View`: one per shown image, holding the stored image size, window size, EXIF orientation, quarter-turns of screen rotation, zoom relative to fit, and the image point at the window centre. That point is kept in image coordinates, so a window resize keeps the same detail centred at the same zoom relative to fit. `View::transform` produces the quad uniform's matrix and offset; orientation and screen rotation compose into one signed-permutation matrix, so mirrored orientations need no special case. Opening an image starts at fit and centred; screen rotation survives navigation away and back, per path, until the app closes (`shell::app` keeps a map from path to `View::turns`, written on each R / L and read when the image is shown again; it survives a new open too).

- Zoom from right-drag or the wheel anchors at the cursor; from keys, at the window centre. Zoom range 1/8× fit to 32× native pixels, except that fit stays reachable when it is already past 32× (a tiny image in a large window).
- Pan clamps so the image cannot leave the window: an image smaller than the window on an axis stays centred on that axis; a larger one keeps covering the window. Every zoom, pan and resize reapplies the clamp.
- R / L return to fit: after a quarter turn the old zoom and pan would land somewhere arbitrary.
- Arrow-key pan moves 10% of the window per press and scrolls the view: Right shows more of the right side.

## Mouse

Built in [keymap.rs](../../src/shell/keymap.rs) as `keymap::Mouse`. A press that moves at most 4 px from where it went down is a click on release; beyond that it is a drag, and the first drag step includes the slop so the image stays under the cursor. Left click previous, right click next; left drag pans, right drag zooms about where the drag started (200 px up doubles). One button at a time: a second button pressed during a gesture is ignored. The wheel zooms 1.25× per notch about the cursor; touchpad pixel scrolling counts 50 px as a notch.

## Advance limit and slideshow

Built as `AdvanceLimit` in [advance.rs](../../src/browse/domain/advance.rs) and `Slideshow` in [slideshow.rs](../../src/browse/domain/slideshow.rs). `App::about_to_wait` ticks both and sleeps until the sooner of their deadlines through `ControlFlow::WaitUntil`.

- A press of Space or Backspace, or a click, always advances. Key repeat counts only as proof the key is still down: a repeat after the limit advances at once; an earlier one is held and advances the moment the limit passes (`AdvanceLimit::due`, which `App::about_to_wait` sleeps until), so the pace is the limit itself, not the limit rounded up to the keyboard's repeat rate. Letting the key go drops a held repeat. A key whose release is never seen advances at most once more, since only a repeat arms the timer.
- Measured: holding Space for 10 s over 24 MP JPEGs advanced 20 times, one every 500 ms. Release build, 50 MP JPEGs, title polled every millisecond: 20 changes in 10.0 s, gaps 484–517 ms, mean 500.8. Wake-ups land up to a frame late, and a late one shortens the next gap by as much, since the clock restarts at `due`: the k-th advance never comes before k limits after the press.
- A toggles the slideshow, with a `Slideshow on` / `Slideshow off` notice; it does nothing before an image list is open. It advances every slideshow interval, wrapping at the end with the usual notice, but only once the next image is in the preload cache (decoded or failed). Past the interval, a next image still decoding holds the current one on screen: no `Loading…`, and no wake-up either, since the decode result is itself an event and `about_to_wait` runs after it. A list of one image never advances.
- Every image change restarts the interval (`Shown::moved`), manual or not, and the interval after a wait is a full one, so each image stays up at least the interval. Sorting does not restart it. An open that replaces the image list stops the slideshow with `Slideshow off`; one that finds nothing leaves it running.
- Measured (debug build, 24 MP JPEGs): changes 10.0 s apart; Space at 25.0 s changed the image and the next change came at 35.0 s. S just before the interval ended put an undecoded image next: the slideshow held the current one 1.1 s past the interval, with no `Loading…`.

## Opening

Built in [browse/infrastructure.rs](../../src/browse/infrastructure.rs): `Open` names the route, `scan` walks it (`walkdir`) and returns a `Found` holding the `ImageList`, the count of unreadable folders (skipped, never fatal) and the paths given to open that do not exist. `App::open` starts every route: command-line paths and drops as `Open::Paths`, O as `Open::Paths` of the picked file, Shift+O as `Open::Recursive`. The result replaces the image list, loads the current image through the preload cache and shows a notice (`12,345 images`, plus `Skipped 3 unreadable folders` or `Not found: …` when they apply).

| Route | Image list |
| --- | --- |
| A file (command line, drop, O) | Images in its folder, not recursive; current image is that file; sort by name. |
| A folder (command line, drop) | Images in it, not recursive; sort by name. |
| Shift+O | Recursive open of the chosen folder; shuffle. |
| Several paths (command line, or drops in one burst) | Each file itself and each folder's images, not recursive, deduplicated; current image is the first image file named; sort by name. |

- Drops: winit sends one `DroppedFile` per file, and a drag delivers all of them before the event loop next goes idle, so `App::about_to_wait` opens whatever arrived since the last idle as one burst.
- O and Shift+O use `rfd`'s native dialogs, parented to the window and opening at the current image's folder; O filters to `image_file::domain::Format::EXTENSIONS`. They are modal, so the event loop waits for them. Shift+O is the Shift key, not the capital letter, so Caps Lock leaves O alone.
- An open that finds no images keeps the current image list; only its notice (`No images found`) shows.
- The folder filter applies to every folder below the opened root, matching folder names case-insensitively; the root itself is never filtered.
- Progress goes out at most every 100 ms, so a scan that finishes sooner shows no `Scanning…` notice. Measured: 100,000 files scanned in 0.3 s warm (release), 1.4 s in a debug build while zooming, with the window redrawing throughout.

## Image list order

[browse/domain.rs](../../src/browse/domain.rs): `ImageList` holds the entries in display order and the current image; `Order` is the sort mode plus reverse and grouping. `ImageList::set_order` re-sorts in memory and keeps the current image, so the screen does not change; the preload cache is reindexed and retargeted at the image's new neighbours.

- N, D and Z set the sort mode and S shuffles anew; reverse and grouping stay as they were. V and G toggle them. Each shows one notice naming the whole order (`browse::presentation::describe`: `Sorted by date, reversed, grouped by folder`), replacing the last.
- Ctrl+Right goes to the first image of the next group; Ctrl+Left to the first image of the group before the current image's, wherever in its group that image is (`ImageList::jump_group`). Both do nothing unless grouped.

- Name sort is natural and case-insensitive (`img2` before `img10`), on the file name only; date is the file's modified time; size is bytes. Ties fall back to name, then the full path, so every order is total.
- Shuffle starts from path order and runs Fisher–Yates on SplitMix64 from the stored `u64` seed, so a seed rebuilds the same order whatever the scan order; a test pins the output, since changing the generator would scramble restored sessions.
- Reverse turns the order within each group round; grouping keeps groups in natural folder-path order either way. A grouped shuffle shuffles inside each group.
- Each entry caches its lowercased file name and folder at scan time. Without that, re-sorting 100,000 images took 330 ms; with it, about 55 ms (release), on the main thread.

## Files and persistence

- `superslide-prefs.toml` and `superslide-session.toml`, both beside `superslide.exe`. Writes go to a temp file, then rename.
- Unwritable or corrupt file: run on defaults, show one notice, keep running. A corrupt file is renamed `.bak` before being replaced.
- Session is saved on exit and at most every 10 s while it has changed.

## File actions

Delete, M and C act on the image on screen, error cards included, even while the current image is still loading ([app.rs](../../src/shell/app.rs) `Shown::act`); the filesystem work is in [file_actions/infrastructure.rs](../../src/file_actions/infrastructure.rs), on the file action thread.

- Delete calls `trash::delete`, with no confirmation: the Recycle Bin is the undo. On a drive with no Recycle Bin (a network share), Windows asks before deleting for good.
- M and C open the native folder picker at the target folder, or the image's own folder before the first one; it is kept until the app closes (SSR-19 persists it).
- Neither ever overwrites. A target name that exists, including the image's own folder, fails with `Failure::Exists` and touches neither file. Moves use `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`, since `std::fs::rename` replaces on Windows; copies create the target with `create_new`, so a file appearing between the check and the write is still never replaced.
- A move between drives copies, flushes the copy to disk, then deletes the original; a failed copy (a full drive) removes its partial file and leaves the original. If only the delete fails, both files stay and the notice says so (`Failure::OriginalKept`).
- Copies keep the original's modified time, so date sort places them alike.
- Every outcome shows a notice naming the file and the target folder's name (not its full path, which wraps over several lines). A held key acts once, and a second press on an image whose action is still running is ignored.
- After a delete or move, `ImageList::remove` takes the image out and `PreloadCache::reindex` drops it from the cache. If it was current, the next image becomes current (wrapping from the last to the first, with the wrap notice) and shows at once when preloaded: the next image is always in the preload window. Removing the last image leaves an empty view (`Showing::Nothing`), the bare window title and `No images left`.
- The stand-in has no file, so file actions on it show `This image is not a file`; the clipboard image (SSR-16) will too.
- Measured on an RTX 3070, release build, 50 MP JPEGs: Delete to the next image on screen 109–126 ms, all of it the Recycle Bin call; a move within a drive and one from C: to D: (17 MB, copied and flushed) put the next image up 315 and 428 ms after the picker closed, with no `Loading…`. A deleted image restored from the Recycle Bin to its folder byte-identical.

## Clipboard

Ctrl+V shows the clipboard image in place of the current image without adding it to the image list; file actions and Ctrl+C are disabled on it, and the next navigation returns to the image list where it left off. Ctrl+C on a file re-decodes it, applies EXIF orientation and screen rotation on the CPU so the copy matches the screen, and puts the RGBA pixels on the clipboard.

## Preference defaults

`Preferences::default` ([prefs/domain.rs](../../src/prefs/domain.rs)). `shell::app` reads the advance limit, slideshow interval, folder filter and preload budget from it until the preferences file (SSR-18) exists.

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
- `cargo run --release --example make_fixtures -- <dir>` ([make_fixtures.rs](../../examples/make_fixtures.rs)) writes the phase 1 acceptance set into a directory outside the repo: 30 numbered 50 MP JPEGs (every fifth portrait) in `50mp/`, and `stripes.png`, the stripe stand-in as a file. It is generated, never committed.
- `wgpu` and `egui-wgpu` build without default features, so only the Dx12 backend compiles; `image` builds only PNG, TGA and BMP, since JPEG goes through `zune-jpeg`; tests add its `jpeg` feature as a dev-dependency to encode JPEGs, which the shipped build does not compile. The defaults would also compile Vulkan, GL, Metal, WebGL, AVIF and WebP support the viewer never uses.
- A scaffolded module carries `#![expect(dead_code, reason = "scaffold: …")]`. Once the module is wired up, the expectation goes unfulfilled and `clippy -D warnings` fails: delete the attribute then.
