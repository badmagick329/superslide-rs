# Vim keymap

Status: Accepted (2026-10-11).

## Context

The bindings were the original SuperSlide's. The owner uses Vim and wanted its habits to work in the viewer (SSR-25). The original's keys stay the default, so a *Keymap* preference chooses between them. The mapping was settled with the owner in chat; the table is in [keymap.md](../context/keymap.md).

## Decision

- **Frequency decides Shift.** Paging is far more common than panning, so lowercase j / k page and H J K L pan. Vim's j-down-means-forward keeps the direction familiar.
- **Vim's motions map onto the image list.** Ctrl+D / Ctrl+U, Vim's half-page moves, jump 10 images. `[count]j` moves that many. `[count]G` and `[count]gg` go to that image, as to a line number. Jumps and counted moves stop at the ends: a wrap partway through a jump lands on an image the owner cannot predict. Single steps still wrap.
- **gg / G are previous / next group**, matching Vim's direction (gg towards the start, G towards the end). Without two or more groups they fall back to the first / last image, which is also what they mean in Vim. With a single group the fallback is needed, or G would wrap to the first image.
- **Esc never quits.** Vim users press Esc out of habit; it clears a count or half-typed key, then leaves fullscreen. q and ZZ quit.
- **ranger's two-key scheme for settings:** `o` + a letter sorts (`on`, `om`, `os`, `oz` random, `or` reverse) and `z` + a letter toggles (`zg` grouping). That freed `e` / `E` for opening, after Vim's `:e`.
- **Vim's own meanings where one exists:** `dd` delete, `y` / `p` yank and paste through the clipboard, `>` / `<` rotate (indent right / left, also sxiv's rotate), `:` preferences (`:set`), Ctrl+G file info, `/` filter. `f` fullscreen comes from sxiv and qutebrowser.
- **Capitals are the Shift key**, not the character, so Caps Lock cannot turn paging into panning, as O / Shift+O already worked.
- **Pending keys never time out**, as Vim's built-in multi-key commands do not. The bottom-right corner shows them, where Vim's `showcmd` does, so a stray `g` is visible.

## Consequences

- Two binding tables in [keymap.md](../context/keymap.md), each held to its `BINDINGS` by the same tests.
- The help overlay, the welcome screen and the preferences screen's notes name the active keymap's keys, through `keymap::Hints`.
- `AdvanceLimit` paces any move, not only a single step, so a held Ctrl+D keeps jumping 10 at the advance limit.
- Classic Ctrl+Left / Ctrl+Right still do nothing when not grouped; only gg / G fall back to the ends.
