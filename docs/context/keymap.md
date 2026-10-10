# Keymap

The v1 bindings; [keymap.rs](../../src/shell/keymap.rs) implements exactly this table, and its `BINDINGS`, which the help overlay lists, repeats it row for row with sample inputs. A test fails when this table and `BINDINGS` differ, or when a sample input gives the wrong command, so change all three together. Rows are in sections (Navigate, View, Order, Files, Clipboard, Window), each one run. Every row works except I (SSR-17) and P (SSR-18). egui sees input first but keeps it only while one of its widgets has focus.

| Input | Command |
| --- | --- |
| Space, right click | Next image |
| Backspace, left click | Previous image |
| Ctrl+Right / Ctrl+Left | First image of the next / previous group (group by subfolder only) |
| A | Start or stop the slideshow |
| Left drag, arrow keys | Pan |
| Right drag, mouse wheel, + / - | Zoom (+ / - and each wheel notch: 1.25×; = also zooms in) |
| Ctrl+ / Ctrl- | Zoom by double / half |
| 0 | Back to fit |
| R / L | Screen rotation right / left |
| Ctrl+F | Toggle the magnification filter between linear and nearest |
| I | Cycle the info overlay |
| N, D, Z | Sort by name, date, size |
| S | Shuffle (again, if already shuffled) |
| V | Reverse the sort |
| G | Toggle group by subfolder |
| O / Shift+O | Open a file / recursive open of a folder |
| Delete | Delete the current image to the Recycle Bin |
| M / C | Move / copy the current image to a folder (picker opens at the last target folder) |
| Ctrl+C / Ctrl+V | Copy the current image to the clipboard / show the clipboard image |
| F11, Enter | Toggle fullscreen |
| Esc | Leave fullscreen; in a window, quit |
| P | Open or close preferences |

Holding Space or Backspace repeats under the advance limit; holding an arrow, + or - repeats at the keyboard's rate; every other key acts once per press, so a held Delete deletes one image. Letter bindings and arrow pan ignore the key while Ctrl or Alt is held, leaving those chords to their own rows. Fullscreen is borderless on the window's current monitor, with no display mode change. Bindings the original had and v1 drops: H (no transitions to skip), t/T (contrast), Alt (no menu). Debug builds add F9, which shows a test notice.
