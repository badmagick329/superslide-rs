# Keymap

Every key and mouse binding, in two keymaps the *Keymap* preference chooses between: classic (the default, the original's keys) and Vim. [keymap/classic.rs](../../src/shell/keymap/classic.rs) and [keymap/vim.rs](../../src/shell/keymap/vim.rs) implement exactly these tables, and each one's `BINDINGS`, which the help overlay lists for the keymap in use, repeats its table row for row with sample inputs. A test fails when a table and its `BINDINGS` differ, or when a sample input gives the wrong command, so change all three together. Rows are in sections (Navigate, View, Order, Marks, Files, Clipboard, Window), each one run; only the Vim keymap has Marks. egui sees input first but keeps it only while one of its widgets has focus. Mouse bindings are the same in both keymaps.

## Classic

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
| Ctrl+Shift+C | Copy the current image's full path, or the marked ones' paths, as text |
| F11, Enter | Toggle fullscreen |
| Esc | Leave fullscreen; in a window, quit |
| P | Open or close preferences |
| ? | Show or hide this list of keys |
| / | Filter this list of keys |

Holding Space or Backspace repeats under the advance limit; holding an arrow, + or - repeats at the keyboard's rate; every other key acts once per press, so a held Delete deletes one image. Letter bindings and arrow pan ignore the key while Ctrl or Alt is held, leaving those chords to their own rows. Fullscreen is borderless on the window's current monitor, with no display mode change. Bindings the original had and v1 drops: H (no transitions to skip), t/T (contrast), Alt (no menu). Debug builds add F9 to both keymaps, which shows a test notice. Only the Vim keymap makes marks, but while any image is marked, Delete, M, C, Ctrl+C and Ctrl+Shift+C here act on every marked image too ([marks](architecture.md#marks)).

While the list of keys (?) shows, it takes every key: arrows, PgUp / PgDn, Home / End and the wheel scroll it, Esc and ? close it, / focuses its filter, and every other binding does nothing until it closes. In the filter, keys type; Esc clears the filter and leaves it, Enter leaves it.

## Vim

Why each key was chosen: [the Vim keymap decision](../decisions/2026-10-11-vim-keymap.md).

| Vim input | Command |
| --- | --- |
| j, Space, right click | Next image |
| k, Backspace, left click | Previous image |
| [count]j / [count]k | That many images forward / back, stopping at the ends |
| Ctrl+D / Ctrl+U | 10 images forward / back, stopping at the ends |
| gg / G | First image of the previous / next group when grouped by subfolder into two or more groups; otherwise the first / last image |
| [count]G, [count]gg | Go to that image: 5G is the fifth |
| a | Start or stop the slideshow |
| H J K L, left drag, arrow keys | Pan |
| Right drag, mouse wheel, + / - | Zoom (+ / - and each wheel notch: 1.25×; = also zooms in) |
| Ctrl+ / Ctrl- | Zoom by double / half |
| 0 | Back to fit |
| > / < | Screen rotation right / left |
| Ctrl+F | Toggle the magnification filter between linear and nearest |
| Ctrl+G | Cycle the info overlay |
| on, om, os | Sort by name, date modified, size |
| oz | Shuffle (again, if already shuffled) |
| or | Reverse the sort |
| zg | Toggle group by subfolder |
| m, [count]m | Mark or unmark the current image, or that many from here: dd, x, c, y and Y then act on every marked image |
| M | Clear all marks |
| n / N | Next / previous marked image |
| e / E | Open a file / recursive open of a folder |
| dd, Delete | Delete the current image, or the marked ones, to the Recycle Bin |
| x / c | Move / copy the current image, or the marked ones, to a folder (picker opens at the last target folder) |
| y, Ctrl+C / p, Ctrl+V | Copy the current image to the clipboard, or the marked ones as files / show the clipboard image |
| Y, Ctrl+Shift+C | Copy the current image's full path, or the marked ones' paths, as text |
| f, F11, Enter | Toggle fullscreen |
| Esc | Clear a count or half-typed key; otherwise leave fullscreen (never quits) |
| q, ZZ | Quit |
| : | Open or close preferences |
| ? | Show or hide this list of keys |
| / | Filter this list of keys |

A capital means the Shift key, not the character, so Caps Lock leaves j paging. `[count]` is digits typed first; 0 alone is fit, and a digit after another digit. Only j, k, Space, Backspace, G, gg and m read a count; any other key acts as usual and drops it. A count or the first key of a two-key binding shows in the bottom-right corner and, as in Vim, never times out: Esc clears it, and a second key that completes no two-key binding is spent with the first, doing nothing. Shift, Ctrl and other modifiers pressed on their own leave it waiting. Skips that stop at an end show `Already at the last image` or `Already at the first image`.

Holding j, k, Space, Backspace, Ctrl+D or Ctrl+U repeats under the advance limit; holding H J K L, an arrow, + or - repeats at the keyboard's rate; every other key acts once per press, and a held key never completes a two-key binding, so a held g is one g. Alt chords do nothing.

While any image is marked, the top-left corner shows how many, and `✔ Marked` over a marked image. How marks behave: [architecture.md](architecture.md#marks) *Marks*; why these keys: [the marks decision](../decisions/2026-10-11-marks.md).

In the list of keys, j / k scroll a line, Ctrl+D / Ctrl+U half a page, gg / G to the ends, as well as the classic keys; Esc first clears a waiting g, then closes it. The preferences screen closes on : or Esc.
