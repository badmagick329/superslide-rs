# The original SuperSlide

Retrieved 2026-10-10 from <https://www.geisswerks.com/superslide/>. Status: complete; the app is unmaintained, so this will not change.

Reference only: the [v1 scope](../decisions/2026-10-10-v1-scope.md) decides what gets built.

## Facts

- Free Windows image viewer by Ryan Geiss (author of MilkDrop), © 2008–2009. Last version 1.00, 2009-06-19; 265 KB download, runs portable from a flash drive.
- Windows 2000, XP and Vista; DirectX 9 with Shader Model 2.0, plus an MMX software mode for machines without a suitable GPU.
- Formats: JPG, PNG (hardware acceleration only), TGA, BMP; Unicode filenames.
- No published source or licence text.

## Behaviour

- Preloads the next images in the background so they appear instantly.
- GPU zoom and pan; pan speed scales with how much of the window the image fills.
- Weighted random shader transitions, including morphing ones, with adjustable speed up to about 5 s.
- Rotation rewrites the EXIF orientation tag and keeps the file timestamp.
- Delete sends files to the Recycle Bin; move and copy are built in.
- Restores the last image, sort mode and shuffle order on startup; remembers window position across monitors.

## Key bindings

| Input | Action |
| --- | --- |
| Space / Backspace | Next / previous image; held, fast-forward / fast-back |
| H | Next image with no transition |
| Right-click / left-click | Next / previous image |
| A | Start or stop slideshow |
| Drag / right-drag | Pan / zoom |
| Arrow keys, + / - | Pan, zoom |
| Ctrl+ / Ctrl- | Zoom by double / half |
| Ctrl+F | Toggle bilinear filtering |
| t / T | Contrast |
| I | Info overlay: none, filename, full details |
| F11 or Enter | Toggle fullscreen; Esc leaves fullscreen |
| R / L | Rotate right / left |
| N, D, Z, S, V | Sort by name, date, size; shuffle; reverse |
| G | Group by subfolder; Ctrl+Left/Right jumps between subfolders |
| O / Shift+O | Open file / open folder recursively |
| Ctrl+C / Ctrl+V | Copy image / view clipboard image |
| P | Preferences |
| Alt | Menu in fullscreen |

## Preferences

Hardware acceleration; disable transitions; block complex shaders ("my video card sucks"); maximum image size in megapixels; family filter (excluded folder names, default `private`); transition speed; restore last image; single or multiple instances; start fullscreen; reset to defaults, including file associations.
