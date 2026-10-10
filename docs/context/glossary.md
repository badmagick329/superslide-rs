# Glossary

The project's ubiquitous language, one row per concept; the index's *Language* section says how to use it. Sorted by term. Default values live in [architecture.md](architecture.md) *Preference defaults*.

| Term | Meaning | Avoid | In code |
| --- | --- | --- | --- |
| Advance limit | The shortest interval between image changes while Space or Backspace is held. | fast-forward, fast-back | `AdvanceLimit` |
| Clipboard image | An image pasted with Ctrl+V, shown in place of the current image but outside the image list; file actions and Ctrl+C are disabled on it. | pasted image | `Showing::Image` with no path while `Shown::pasted` is set |
| Current image | The image on screen, one position in the image list. | current slide | |
| Error card | What is drawn in place of an image that failed to decode: the file name and a one-line reason. | | `load::presentation::error_card` |
| File action | Delete to the Recycle Bin (Delete), move (M) or copy (C) of the current image's file; the only writes the viewer makes to image files. | | `file_actions::domain::Action` |
| Fit | The zoom level at which the whole image fits the window; every image opens at fit. | | |
| Folder filter | Folder names excluded when opening a folder. | family filter, excluded folders | |
| Group by subfolder | Ordering that keeps each subfolder's images together, sorted by the sort mode within each group; Ctrl+Left/Right jumps between groups. | | `Order::grouped`, `ImageList::jump_group` |
| Help overlay | The panel ? opens over the image, listing every binding, scrollable and filterable with /. While open it takes all input. | cheat sheet, shortcuts screen | `help` slice; rows are `shell::keymap::BINDINGS` |
| Image | A JPG, PNG, TGA or BMP file the viewer can show. | photo, slide, picture | |
| Image list | The ordered images from the last open, after the folder filter, sort mode and grouping are applied. | playlist | `ImageList`; its order is `Order` |
| Info overlay | Text in the bottom-left corner describing the image on screen; I cycles it through hidden, file name only and full details. | | `info_overlay` slice; its mode is `InfoOverlay`, kept in `Preferences` |
| Magnification filter | How pixels are drawn when zoomed past native size: linear (smooth) or nearest (sharp); Ctrl+F toggles it. | bilinear filtering | |
| Notice | A short text shown over the image that fades after a few seconds, for errors, progress and confirmations. | toast, popup | |
| Pan | Moving a zoomed image within the window by drag or arrow keys. | scroll | |
| Preferences | The owner's persisted options, and the screen opened with P that edits them. | settings, config, options | `Preferences` |
| Preload cache | Decoded images held on the GPU under a memory budget so navigation shows them without waiting. | | `PreloadCache` |
| Preload window | The images the preload cache wants: the current image, 3 ahead and 1 behind in the direction of travel. | | `preload_window` |
| Recursive open | Opening a folder together with all its subfolders (Shift+O); sets the sort mode to shuffle. | | |
| Screen rotation | Turning how the current image is drawn by 90°. Never written to the file; lasts until the app closes. | lossless rotation, EXIF rotation | |
| Session restore | On startup, reopening the last image list, current image, sort mode, shuffle order and window placement. | | `Session` |
| Shuffle | The sort mode that gives the image list a random order, kept until the next shuffle. | random | |
| Slideshow | Timed automatic advance to the next image, toggled with A. | | `Slideshow` |
| Sort mode | The image list's order: name (N), date (D), size (Z) or shuffle (S); V reverses it. | | `SortMode` |
| Target folder | The folder M or C last moved or copied an image to; the next folder picker opens there. | last target, destination | `Shown::target_folder` |
| Zoom | Scaling the image relative to fit, by right-drag, wheel, + / - or Ctrl+ / Ctrl- (double or half). | | |
