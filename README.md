# SuperSlide-RS

A fast, keyboard-driven image viewer for Windows. Open a folder, then page through it with Space.

## Download

Get `superslide.exe` from [Releases](../../releases/latest). It's portable.

Runs on Windows 10 and 11 (64-bit).

## Use

Open images by dragging a file or folder onto the window, or press **O**. Next time it opens where you left off.

| Key | Does |
| --- | --- |
| Space / Backspace | Next / previous image (or right / left click) |
| A | Start or stop the slideshow |
| Mouse wheel, + / - | Zoom |
| Drag, arrow keys | Pan |
| 0 | Fit to window |
| R / L | Rotate right / left (on screen only, the file is never changed) |
| Enter, F11 | Fullscreen |
| Esc | Leave fullscreen, or quit |
| N, D, Z, S | Sort by name, date, size, or shuffle |
| V | Reverse the order |
| O / Shift+O | Open a file / a folder and all its subfolders |
| Delete | Send the image to the Recycle Bin |
| M / C | Move / copy the image to a folder |
| Ctrl+C / Ctrl+V | Copy the image / show the image on the clipboard |
| I | Show the file name, then full details, then nothing |
| P | Preferences, saved next to `superslide.exe` |
| ? | List every key; / filters the list |

Prefer Vim keys? Set **Keymap** to **Vim** in the preferences: j / k page, H J K L pan, `5j`, `gg` / `G`, `dd`, and Esc never quits.

Move and copy never overwrite a file: if the folder already has one with that name, nothing happens and you get a notice.

[All keys](docs/context/keymap.md)

## Build from source

Needs [Rust](https://rustup.rs) 1.97 or newer.

```
cargo build --release
```

## License

[MIT](LICENSE)
