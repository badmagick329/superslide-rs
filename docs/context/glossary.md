# Glossary

The project's ubiquitous language, one row per concept; the index's *Language* section says how to use it. Sorted by term. Default values live in [architecture.md](architecture.md) *Preference defaults*. Keys named here are the classic keymap's; [keymap.md](keymap.md) gives the Vim ones.

| Term | Meaning | Avoid | In code |
| --- | --- | --- | --- |
| Advance limit | The shortest interval between image changes while a key that moves through the image list is held: Space or Backspace, or Vim's j, k, Ctrl+D or Ctrl+U. | fast-forward, fast-back | `AdvanceLimit` |
| Chosen order | The order last set with a sort key (N, D, Z, S, V or G); every open takes it, a shuffle shuffled anew. Kept in the session. | default sort, last sort | `Shown::chosen_order`; applied by `Open::order` |
| Clipboard image | An image pasted with Ctrl+V, shown in place of the current image but outside the image list; file actions, marks and Ctrl+C are disabled on it. | pasted image | `Showing::Image` with no path while `Shown::pasted` is set |
| Count | Digits typed before a Vim key to say how many or which: `5j` moves five images, `5G` goes to the fifth. | prefix, number argument | `vim::Vim::count` |
| Current image | The image on screen, one position in the image list. | current slide | |
| Error card | What is drawn in place of an image that failed to decode: the file name and a one-line reason. | | `load::presentation::error_card` |
| File action | Delete to the Recycle Bin (Delete), move (M) or copy (C) of the current image's file, or of every marked image's at once; the only writes the viewer makes to image files. | bulk action | `file_actions::domain::Action`; over the marked images, `Batch` |
| Fit | The zoom level at which the whole image fits the window; every image opens at fit. | | |
| Group by subfolder | Ordering that keeps each subfolder's images together, sorted by the sort mode within each group; Ctrl+Left/Right (Vim: gg / G) jumps between groups. | | `Order::grouped`, `ImageList::jump_group` |
| Help overlay | The panel ? opens over the image, listing every binding, scrollable and filterable with /. While open it takes all input. | cheat sheet, shortcuts screen | `help` slice; rows are the keymap's `BINDINGS` |
| Image | A JPG, PNG, TGA or BMP file the viewer can show. | photo, slide, picture | |
| Image list | The ordered images from the last open, without skipped folders, after the sort mode and grouping are applied. | playlist | `ImageList`; its order is `Order` |
| Info overlay | Text in the bottom-left corner describing the image on screen; I cycles it through hidden, file name only and full details. | | `info_overlay` slice; its mode is `InfoOverlay`, kept in `Preferences` |
| Keymap | The set of key bindings in use, a preference: classic (the original's keys, the default) or Vim. Mouse bindings are the same in both. | key scheme, shortcuts, Vim mode | `prefs::domain::Keymap`; run by `shell::keymap::Keys` |
| Mark | A flag on an image, set with Vim's m, that makes file actions and Ctrl+C act on every marked image at once. It belongs to the file, so re-sorting keeps it; a new image list starts with none. | selection, tag, select | `ImageList::marked` |
| Magnification filter | How pixels are drawn when zoomed past native size: linear (smooth) or nearest (sharp); Ctrl+F toggles it. | bilinear filtering | |
| Notice | A short text shown over the image that fades after a few seconds, for errors, progress and confirmations. | toast, popup | |
| Pan | Moving a zoomed image within the window by drag or arrow keys. | scroll | |
| Pending keys | A count or the first key of a two-key Vim binding, shown in the bottom-right corner until the binding completes or Esc clears it. | | `Keys::pending` |
| Preferences | The owner's persisted options, kept in `superslide-prefs.toml` beside the executable, and the screen opened with P (Vim: `:`) that edits them. | settings, config, options | `prefs` slice; the options are `Preferences`, the file `TomlFile<Preferences>`, the open screen `Screen` |
| Preload cache | Decoded images held on the GPU under a memory budget so navigation shows them without waiting. | | `PreloadCache` |
| Preload window | The images the preload cache wants: the current image, 3 ahead and 1 behind in the direction of travel. | | `preload_window` |
| Recursive open | Opening a folder together with all its subfolders (Shift+O); takes the chosen order, or shuffles when none is chosen. | | |
| Screen rotation | Turning how the current image is drawn by 90°. Never written to the file; lasts until the app closes. | lossless rotation, EXIF rotation | |
| Session restore | On startup, reopening the last image list, current image, sort mode, shuffle order and window placement, and bringing back the chosen order, kept in `superslide-session.toml` beside the executable. | | `Session`; where the window reopens, `Placement` |
| Shuffle | The sort mode that gives the image list a random order, kept until the next shuffle. | random | |
| Skipped folders | Folder names a recursive open leaves out, with everything inside them; names match whole and in any case. The folder opened is never skipped. A preference. | folder filter, family filter, excluded folders | `Preferences::folder_filter`, the same key in the preferences file |
| Slideshow | Timed automatic advance to the next image, toggled with A. | | `Slideshow` |
| Sort mode | The image list's order: name (N), date (D), size (Z) or shuffle (S); V reverses it. | | `SortMode` |
| Target folder | The folder M or C last moved or copied an image to; the next folder picker opens there. | last target, destination | `Shown::target_folder` |
| Welcome screen | What the window shows while no image list is open: the app icon, the keys that open images, and a drop hint. | intro screen, splash screen, empty state | `welcome` slice; shown when `Shown::welcoming` |
| Zoom | Scaling the image relative to fit, by right-drag, wheel, + / - or Ctrl+ / Ctrl- (double or half). | | |
