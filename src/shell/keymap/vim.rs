//! The Vim keymap: j / k move through the image list and H J K L pan, since paging is far more
//! common than panning; two-key bindings (`gg`, `dd`, `on`, `zg`, `ZZ`) and counts (`5j`, `5G`)
//! as in Vim. Why each key was chosen: `docs/decisions/2026-10-11-vim-keymap.md`.

use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::{
    CLICK_NEXT, CLICK_PREVIOUS, DRAG_PAN, DRAG_ZOOM, HelpKey, Hints, Input, Probe, Row, WHEEL_ZOOM,
    ZOOM_STEP, advance, help_scroll, pan_key, row, zoom,
};
use crate::browse::domain::{Direction, SortKey, Travel};
use crate::file_actions::domain::Transfer;
use crate::help::domain::{Scroll, Section};
use crate::prefs::presentation::PreferenceKeys;
use crate::shell::command::Command;
use crate::view::domain::Turn;
use crate::welcome::presentation::OpeningKeys;

/// Images Ctrl+D and Ctrl+U move.
const JUMP: usize = 10;
/// Above any image list's length, and small enough that typing more digits cannot overflow.
const MAX_COUNT: usize = 9_999_999;

pub const HINTS: Hints = Hints {
    openings: OpeningKeys {
        file: "e",
        folder: "E",
        help: "?",
    },
    help: "j / k, Ctrl+D / Ctrl+U, gg / G, wheel, arrows scroll · Esc closes",
    preferences: PreferenceKeys {
        close: ": or Esc",
        advance: "Held j / k, Space, Backspace",
        magnification: "Ctrl+F",
        info: "Ctrl+G",
    },
};

/// What has been typed towards a binding: a count, the first key of a two-key binding, or both,
/// as in `5g` on the way to `5gg`. As in Vim, neither times out; the corner shows them, and Esc
/// or a key that completes nothing clears them.
#[derive(Debug, Default)]
pub struct Vim {
    count: Option<usize>,
    prefix: Option<Prefix>,
}

/// The first key of a two-key binding.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Prefix {
    /// `gg`
    G,
    /// `dd`
    D,
    /// `on`, `om`, `os`, `oz`, `or`
    O,
    /// `zg`
    Z,
    /// `ZZ`
    ShiftZ,
}

/// A key press as the Vim keymap reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stroke {
    /// Letters are lowercase, with `shift` from the Shift key, so Caps Lock changes nothing.
    /// Any other character is as typed, Shift already in it, and `shift` is false.
    Char {
        c: char,
        shift: bool,
        ctrl: bool,
    },
    Named {
        key: NamedKey,
        ctrl: bool,
    },
    /// A key with no binding: Alt chords, dead keys, unidentified keys.
    Other,
}

impl Stroke {
    /// `None` for a modifier pressed on its own, which must not cancel a count or a first key:
    /// `ZZ` and `5G` hold Shift between their keys.
    fn of(key: &Key, modifiers: ModifiersState) -> Option<Self> {
        let ctrl = modifiers.control_key();
        if let Key::Named(
            NamedKey::Shift
            | NamedKey::Control
            | NamedKey::Alt
            | NamedKey::AltGraph
            | NamedKey::Super
            | NamedKey::Meta
            | NamedKey::Hyper
            | NamedKey::Fn
            | NamedKey::CapsLock
            | NamedKey::NumLock,
        ) = key
        {
            return None;
        }
        if modifiers.alt_key() {
            return Some(Stroke::Other);
        }
        Some(match key {
            Key::Named(key) => Stroke::Named { key: *key, ctrl },
            Key::Character(text) => {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if c.is_ascii_alphabetic() => Stroke::Char {
                        c: c.to_ascii_lowercase(),
                        shift: modifiers.shift_key(),
                        ctrl,
                    },
                    (Some(c), None) => Stroke::Char {
                        c,
                        shift: false,
                        ctrl,
                    },
                    _ => Stroke::Other,
                }
            }
            _ => Stroke::Other,
        })
    }

    const fn plain(c: char) -> Self {
        Stroke::Char {
            c,
            shift: false,
            ctrl: false,
        }
    }

    const fn shifted(c: char) -> Self {
        Stroke::Char {
            c,
            shift: true,
            ctrl: false,
        }
    }
}

impl Vim {
    pub fn pressed(
        &mut self,
        key: &Key,
        modifiers: ModifiersState,
        repeat: bool,
    ) -> Option<Command> {
        let stroke = Stroke::of(key, modifiers)?;
        if repeat {
            // A held key repeats only bindings that repeat, and never completes or extends what
            // is waiting: a held g must not make gg.
            if self.pending().is_some() {
                return None;
            }
            return single(stroke, None, true)
                .and_then(|(command, repeats)| repeats.then_some(command));
        }
        if stroke
            == (Stroke::Named {
                key: NamedKey::Escape,
                ctrl: false,
            })
        {
            return match self.pending() {
                Some(_) => {
                    *self = Self::default();
                    None
                }
                None => Some(Command::LeaveFullscreen),
            };
        }
        if let Some(prefix) = self.prefix.take() {
            return second(prefix, self.count.take(), stroke);
        }
        if let Stroke::Char {
            c,
            shift: false,
            ctrl: false,
        } = stroke
        {
            // 0 alone is fit; after another digit it is part of the count, as in Vim.
            if let Some(digit) = c.to_digit(10)
                && (digit != 0 || self.count.is_some())
            {
                let count = self.count.unwrap_or(0) * 10 + digit as usize;
                self.count = Some(count.min(MAX_COUNT));
                return None;
            }
        }
        let prefix = match stroke {
            s if s == Stroke::plain('g') => Some(Prefix::G),
            s if s == Stroke::plain('d') => Some(Prefix::D),
            s if s == Stroke::plain('o') => Some(Prefix::O),
            s if s == Stroke::plain('z') => Some(Prefix::Z),
            s if s == Stroke::shifted('z') => Some(Prefix::ShiftZ),
            _ => None,
        };
        if prefix.is_some() {
            self.prefix = prefix;
            return None;
        }
        single(stroke, self.count.take(), false).map(|(command, _)| command)
    }

    /// The help overlay's keys: the shared scroll keys plus j / k, Ctrl+D / Ctrl+U and gg / G.
    pub fn help_key(
        &mut self,
        key: &Key,
        modifiers: ModifiersState,
        repeat: bool,
    ) -> Option<HelpKey> {
        if let Some(scroll) = help_scroll(key) {
            return Some(HelpKey::Scroll(scroll));
        }
        let stroke = Stroke::of(key, modifiers)?;
        let scroll = match stroke {
            s if s == Stroke::plain('j') => Some(Scroll::Lines(1.0)),
            s if s == Stroke::plain('k') => Some(Scroll::Lines(-1.0)),
            Stroke::Char {
                c: 'd', ctrl: true, ..
            } => Some(Scroll::Pages(0.5)),
            Stroke::Char {
                c: 'u', ctrl: true, ..
            } => Some(Scroll::Pages(-0.5)),
            _ => None,
        };
        // Only scrolls repeat, and a held key leaves a waiting g alone.
        if repeat {
            return scroll.map(HelpKey::Scroll);
        }
        let g_waiting = self.prefix.take() == Some(Prefix::G);
        self.count = None;
        if let Some(scroll) = scroll {
            return Some(HelpKey::Scroll(scroll));
        }
        Some(match stroke {
            s if s == Stroke::plain('g') && g_waiting => HelpKey::Scroll(Scroll::Top),
            s if s == Stroke::plain('g') => {
                self.prefix = Some(Prefix::G);
                return None;
            }
            s if s == Stroke::shifted('g') => HelpKey::Scroll(Scroll::Bottom),
            // As outside the overlay, Esc first clears what waits.
            Stroke::Named {
                key: NamedKey::Escape,
                ..
            } if g_waiting => return None,
            Stroke::Named {
                key: NamedKey::Escape,
                ..
            } => HelpKey::Close,
            s if s == Stroke::plain('?') => HelpKey::Close,
            s if s == Stroke::plain('/') => HelpKey::FocusFilter,
            _ => return None,
        })
    }

    pub fn pending(&self) -> Option<String> {
        let count = self.count.map(|count| count.to_string());
        let prefix = self.prefix.map(|prefix| match prefix {
            Prefix::G => "g",
            Prefix::D => "d",
            Prefix::O => "o",
            Prefix::Z => "z",
            Prefix::ShiftZ => "Z",
        });
        match (count, prefix) {
            (None, None) => None,
            (count, prefix) => Some(count.unwrap_or_default() + prefix.unwrap_or_default()),
        }
    }
}

/// The second key of a two-key binding; any key that completes none gives nothing, and both
/// keys are spent, as in Vim.
fn second(prefix: Prefix, count: Option<usize>, stroke: Stroke) -> Option<Command> {
    let Stroke::Char {
        c,
        shift,
        ctrl: false,
    } = stroke
    else {
        return None;
    };
    Some(match (prefix, c, shift) {
        (Prefix::G, 'g', false) => match count {
            Some(position) => Command::GoTo(position),
            None => Command::JumpGroupOrEnd(Direction::Backward),
        },
        (Prefix::D, 'd', false) => Command::DeleteToRecycleBin,
        (Prefix::O, 'n', false) => Command::Sort(SortKey::Name),
        (Prefix::O, 'm', false) => Command::Sort(SortKey::Date),
        (Prefix::O, 's', false) => Command::Sort(SortKey::Size),
        (Prefix::O, 'z', false) => Command::Shuffle,
        (Prefix::O, 'r', false) => Command::ReverseSort,
        (Prefix::Z, 'g', false) => Command::ToggleGrouping,
        (Prefix::ShiftZ, 'z', true) => Command::Quit,
        _ => return None,
    })
}

/// A one-key binding and whether it repeats while held. Only j / k, Space / Backspace, G, gg and
/// m read the count; every other key drops it.
fn single(stroke: Stroke, count: Option<usize>, repeat: bool) -> Option<(Command, bool)> {
    // Repeat passes through: the advance limit, not the keyboard, sets the pace.
    let travel = |direction| {
        let travel = match count {
            Some(images) => Travel::Skip(direction, images),
            None => Travel::Step(direction),
        };
        (Command::Advance { travel, repeat }, true)
    };
    Some(match stroke {
        Stroke::Named { key, ctrl: false } => match key {
            NamedKey::Space => travel(Direction::Forward),
            NamedKey::Backspace => travel(Direction::Backward),
            NamedKey::Delete => (Command::DeleteToRecycleBin, false),
            NamedKey::Enter | NamedKey::F11 => (Command::ToggleFullscreen, false),
            #[cfg(debug_assertions)]
            NamedKey::F9 => (Command::TestNotice, false),
            NamedKey::ArrowLeft
            | NamedKey::ArrowRight
            | NamedKey::ArrowUp
            | NamedKey::ArrowDown => (pan_key(key), true),
            _ => return None,
        },
        Stroke::Char {
            c,
            shift,
            ctrl: true,
        } => match c {
            'd' | 'u' => {
                let direction = if c == 'd' {
                    Direction::Forward
                } else {
                    Direction::Backward
                };
                let travel = Travel::Skip(direction, JUMP);
                (Command::Advance { travel, repeat }, true)
            }
            '+' | '=' => (zoom(2.0), true),
            '-' => (zoom(0.5), true),
            'f' => (Command::ToggleMagnificationFilter, false),
            'g' => (Command::CycleInfoOverlay, false),
            // As in the classic keymap: Explorer's "Copy as path" key.
            'c' if shift => (Command::CopyPathToClipboard, false),
            'c' => (Command::CopyToClipboard, false),
            'v' => (Command::PasteFromClipboard, false),
            _ => return None,
        },
        Stroke::Char {
            c, shift: false, ..
        } => match c {
            'j' => travel(Direction::Forward),
            'k' => travel(Direction::Backward),
            'a' => (Command::ToggleSlideshow, false),
            '+' | '=' => (zoom(ZOOM_STEP), true),
            '-' => (zoom(1.0 / ZOOM_STEP), true),
            '0' => (Command::Fit, false),
            '>' => (Command::Rotate(Turn::Right), false),
            '<' => (Command::Rotate(Turn::Left), false),
            'e' => (Command::Open { recursive: false }, false),
            'm' => (Command::ToggleMarks(count.unwrap_or(1)), false),
            'n' => (Command::NextMarked(Direction::Forward), false),
            'x' => (Command::Transfer(Transfer::Move), false),
            'c' => (Command::Transfer(Transfer::Copy), false),
            'y' => (Command::CopyToClipboard, false),
            'p' => (Command::PasteFromClipboard, false),
            'f' => (Command::ToggleFullscreen, false),
            'q' => (Command::Quit, false),
            ':' => (Command::TogglePreferences, false),
            '?' => (Command::ToggleHelp, false),
            '/' => (Command::FocusHelpFilter, false),
            _ => return None,
        },
        Stroke::Char { c, shift: true, .. } => match c {
            'h' => (pan_key(NamedKey::ArrowLeft), true),
            'j' => (pan_key(NamedKey::ArrowDown), true),
            'k' => (pan_key(NamedKey::ArrowUp), true),
            'l' => (pan_key(NamedKey::ArrowRight), true),
            'g' => match count {
                Some(position) => (Command::GoTo(position), false),
                None => (Command::JumpGroupOrEnd(Direction::Forward), false),
            },
            'e' => (Command::Open { recursive: true }, false),
            'm' => (Command::ClearMarks, false),
            'n' => (Command::NextMarked(Direction::Backward), false),
            'y' => (Command::CopyPathToClipboard, false),
            _ => return None,
        },
        Stroke::Named { ctrl: true, .. } | Stroke::Other => return None,
    })
}

/// : and Esc close the preferences screen.
pub fn closes_preferences(key: &Key, modifiers: ModifiersState, repeat: bool) -> bool {
    let stroke = Stroke::of(key, modifiers);
    let escape = matches!(
        stroke,
        Some(Stroke::Named {
            key: NamedKey::Escape,
            ..
        })
    );
    !repeat && (escape || stroke == Some(Stroke::plain(':')))
}

pub fn released(key: &Key) -> Option<Direction> {
    match key {
        Key::Named(NamedKey::Space) => Some(Direction::Forward),
        Key::Named(NamedKey::Backspace) => Some(Direction::Backward),
        // Shift and Ctrl may come up first, so the letter alone says which key it was.
        Key::Character(c) => match c.to_ascii_lowercase().as_str() {
            "j" | "d" => Some(Direction::Forward),
            "k" | "u" => Some(Direction::Backward),
            _ => None,
        },
        _ => None,
    }
}

const fn keys(keys: &'static str, command: Command) -> Probe {
    Probe(Input::Keys(keys), command)
}

const fn skip(direction: Direction, images: usize) -> Command {
    Command::Advance {
        travel: Travel::Skip(direction, images),
        repeat: false,
    }
}

/// Every binding, in the order of `docs/context/keymap.md`'s Vim table, each section in one run.
pub const BINDINGS: &[Row] = &[
    row(
        "j, Space, right click",
        "Next image",
        Section::Navigate,
        &[
            keys("j", advance(Direction::Forward, false)),
            keys("<Space>", advance(Direction::Forward, false)),
            CLICK_NEXT,
        ],
    ),
    row(
        "k, Backspace, left click",
        "Previous image",
        Section::Navigate,
        &[
            keys("k", advance(Direction::Backward, false)),
            keys("<BS>", advance(Direction::Backward, false)),
            CLICK_PREVIOUS,
        ],
    ),
    row(
        "[count]j / [count]k",
        "That many images forward / back, stopping at the ends",
        Section::Navigate,
        &[
            keys("5j", skip(Direction::Forward, 5)),
            keys("12k", skip(Direction::Backward, 12)),
            keys("3<Space>", skip(Direction::Forward, 3)),
        ],
    ),
    row(
        "Ctrl+D / Ctrl+U",
        "10 images forward / back, stopping at the ends",
        Section::Navigate,
        &[
            keys("<C-d>", skip(Direction::Forward, JUMP)),
            keys("<C-u>", skip(Direction::Backward, JUMP)),
        ],
    ),
    row(
        "gg / G",
        "First image of the previous / next group when grouped by subfolder into two or more groups; otherwise the first / last image",
        Section::Navigate,
        &[
            keys("gg", Command::JumpGroupOrEnd(Direction::Backward)),
            keys("G", Command::JumpGroupOrEnd(Direction::Forward)),
        ],
    ),
    row(
        "[count]G, [count]gg",
        "Go to that image: 5G is the fifth",
        Section::Navigate,
        &[
            keys("5G", Command::GoTo(5)),
            keys("20gg", Command::GoTo(20)),
        ],
    ),
    row(
        "a",
        "Start or stop the slideshow",
        Section::Navigate,
        &[keys("a", Command::ToggleSlideshow)],
    ),
    row(
        "H J K L, left drag, arrow keys",
        "Pan",
        Section::View,
        &[
            keys("H", pan_key(NamedKey::ArrowLeft)),
            keys("J", pan_key(NamedKey::ArrowDown)),
            keys("K", pan_key(NamedKey::ArrowUp)),
            keys("L", pan_key(NamedKey::ArrowRight)),
            DRAG_PAN,
            keys("<Left>", pan_key(NamedKey::ArrowLeft)),
        ],
    ),
    row(
        "Right drag, mouse wheel, + / -",
        "Zoom (+ / - and each wheel notch: 1.25×; = also zooms in)",
        Section::View,
        &[
            DRAG_ZOOM,
            WHEEL_ZOOM,
            keys("+", zoom(ZOOM_STEP)),
            keys("=", zoom(ZOOM_STEP)),
            keys("-", zoom(1.0 / ZOOM_STEP)),
        ],
    ),
    row(
        "Ctrl+ / Ctrl-",
        "Zoom by double / half",
        Section::View,
        &[
            keys("<C-+>", zoom(2.0)),
            keys("<C-=>", zoom(2.0)),
            keys("<C-->", zoom(0.5)),
        ],
    ),
    row(
        "0",
        "Back to fit",
        Section::View,
        &[keys("0", Command::Fit)],
    ),
    row(
        "> / <",
        "Screen rotation right / left",
        Section::View,
        &[
            keys(">", Command::Rotate(Turn::Right)),
            keys("<lt>", Command::Rotate(Turn::Left)),
        ],
    ),
    row(
        "Ctrl+F",
        "Toggle the magnification filter between linear and nearest",
        Section::View,
        &[keys("<C-f>", Command::ToggleMagnificationFilter)],
    ),
    row(
        "Ctrl+G",
        "Cycle the info overlay",
        Section::View,
        &[keys("<C-g>", Command::CycleInfoOverlay)],
    ),
    row(
        "on, om, os",
        "Sort by name, date modified, size",
        Section::Order,
        &[
            keys("on", Command::Sort(SortKey::Name)),
            keys("om", Command::Sort(SortKey::Date)),
            keys("os", Command::Sort(SortKey::Size)),
        ],
    ),
    row(
        "oz",
        "Shuffle (again, if already shuffled)",
        Section::Order,
        &[keys("oz", Command::Shuffle)],
    ),
    row(
        "or",
        "Reverse the sort",
        Section::Order,
        &[keys("or", Command::ReverseSort)],
    ),
    row(
        "zg",
        "Toggle group by subfolder",
        Section::Order,
        &[keys("zg", Command::ToggleGrouping)],
    ),
    row(
        "m, [count]m",
        "Mark or unmark the current image, or that many from here: dd, x, c, y and Y then act on every marked image",
        Section::Marks,
        &[
            keys("m", Command::ToggleMarks(1)),
            keys("5m", Command::ToggleMarks(5)),
        ],
    ),
    row(
        "M",
        "Clear all marks",
        Section::Marks,
        &[keys("M", Command::ClearMarks)],
    ),
    row(
        "n / N",
        "Next / previous marked image",
        Section::Marks,
        &[
            keys("n", Command::NextMarked(Direction::Forward)),
            keys("N", Command::NextMarked(Direction::Backward)),
        ],
    ),
    row(
        "e / E",
        "Open a file / recursive open of a folder",
        Section::Files,
        &[
            keys("e", Command::Open { recursive: false }),
            keys("E", Command::Open { recursive: true }),
        ],
    ),
    row(
        "dd, Delete",
        "Delete the current image, or the marked ones, to the Recycle Bin",
        Section::Files,
        &[
            keys("dd", Command::DeleteToRecycleBin),
            keys("<Del>", Command::DeleteToRecycleBin),
        ],
    ),
    row(
        "x / c",
        "Move / copy the current image, or the marked ones, to a folder (picker opens at the last target folder)",
        Section::Files,
        &[
            keys("x", Command::Transfer(Transfer::Move)),
            keys("c", Command::Transfer(Transfer::Copy)),
        ],
    ),
    row(
        "y, Ctrl+C / p, Ctrl+V",
        "Copy the current image to the clipboard, or the marked ones as files / show the clipboard image",
        Section::Clipboard,
        &[
            keys("y", Command::CopyToClipboard),
            keys("<C-c>", Command::CopyToClipboard),
            keys("p", Command::PasteFromClipboard),
            keys("<C-v>", Command::PasteFromClipboard),
        ],
    ),
    row(
        "Y, Ctrl+Shift+C",
        "Copy the current image's full path, or the marked ones' paths, as text",
        Section::Clipboard,
        &[
            keys("Y", Command::CopyPathToClipboard),
            keys("<C-S-c>", Command::CopyPathToClipboard),
        ],
    ),
    row(
        "f, F11, Enter",
        "Toggle fullscreen",
        Section::Window,
        &[
            keys("f", Command::ToggleFullscreen),
            keys("<F11>", Command::ToggleFullscreen),
            keys("<CR>", Command::ToggleFullscreen),
        ],
    ),
    row(
        "Esc",
        "Clear a count or half-typed key; otherwise leave fullscreen (never quits)",
        Section::Window,
        &[keys("<Esc>", Command::LeaveFullscreen)],
    ),
    row(
        "q, ZZ",
        "Quit",
        Section::Window,
        &[keys("q", Command::Quit), keys("ZZ", Command::Quit)],
    ),
    row(
        ":",
        "Open or close preferences",
        Section::Window,
        &[keys(":", Command::TogglePreferences)],
    ),
    row(
        "?",
        "Show or hide this list of keys",
        Section::Window,
        &[keys("?", Command::ToggleHelp)],
    ),
    row(
        "/",
        "Filter this list of keys",
        Section::Window,
        &[keys("/", Command::FocusHelpFilter)],
    ),
    // Last, so the rows above stay the doc's table in every build.
    #[cfg(debug_assertions)]
    row(
        "F9",
        "Show a test notice",
        Section::Window,
        &[keys("<F9>", Command::TestNotice)],
    ),
];

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::shell::keymap::tests::replay_mouse;
    use crate::view::domain::Anchor;

    /// Presses, in Vim's notation: a capital letter is Shift and that letter; `<C-x>` is Ctrl and
    /// x; `<C-S-x>` is Ctrl, Shift and x, capitalised as Shift makes it; `<Space>`, `<BS>`,
    /// `<Del>`, `<CR>`, `<Esc>`, `<Left>`, `<F11>`, `<F9>` and `<lt>` (a literal <) are those keys.
    fn presses(notation: &str) -> Vec<(Key, ModifiersState)> {
        let mut presses = Vec::new();
        let mut rest = notation;
        while let Some(c) = rest.chars().next() {
            let (press, len) = match rest.find('>').filter(|_| c == '<' && rest.len() > 1) {
                Some(end) => (special(&rest[1..end]), end + 1),
                None if c.is_ascii_uppercase() => (
                    (Key::Character(c.to_string().into()), ModifiersState::SHIFT),
                    1,
                ),
                None => (
                    (
                        Key::Character(c.to_string().into()),
                        ModifiersState::empty(),
                    ),
                    c.len_utf8(),
                ),
            };
            presses.push(press);
            rest = &rest[len..];
        }
        presses
    }

    fn special(name: &str) -> (Key, ModifiersState) {
        let none = ModifiersState::empty();
        if let Some(c) = name.strip_prefix("C-S-") {
            let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
            return (Key::Character(c.to_uppercase().into()), ctrl_shift);
        }
        if let Some(c) = name.strip_prefix("C-") {
            return (Key::Character(c.into()), ModifiersState::CONTROL);
        }
        let key = match name {
            "lt" => return (Key::Character("<".into()), none),
            "Space" => NamedKey::Space,
            "BS" => NamedKey::Backspace,
            "Del" => NamedKey::Delete,
            "CR" => NamedKey::Enter,
            "Esc" => NamedKey::Escape,
            "Left" => NamedKey::ArrowLeft,
            "F11" => NamedKey::F11,
            "F9" => NamedKey::F9,
            _ => panic!("no such key: <{name}>"),
        };
        (Key::Named(key), none)
    }

    /// Commands each press of `notation` gives, from a fresh keymap.
    fn typed(notation: &str) -> Vec<Option<Command>> {
        let mut vim = Vim::default();
        presses(notation)
            .iter()
            .map(|(key, modifiers)| vim.pressed(key, *modifiers, false))
            .collect()
    }

    pub fn replay(input: &Input) -> Option<Command> {
        if let Some(command) = replay_mouse(input) {
            return command;
        }
        let Input::Keys(notation) = *input else {
            panic!("Vim probes are keys in Vim's notation");
        };
        let mut commands = typed(notation);
        let last = commands.pop().flatten();
        assert!(
            commands.iter().all(Option::is_none),
            "{notation}: only the last press may act"
        );
        last
    }

    /// The row each command belongs to; exhaustive, as in the classic keymap's.
    pub fn row_of(command: Command) -> &'static str {
        match command {
            Command::Advance {
                travel: Travel::Step(Direction::Forward),
                ..
            } => "j, Space, right click",
            Command::Advance {
                travel: Travel::Step(Direction::Backward),
                ..
            } => "k, Backspace, left click",
            Command::Advance {
                travel: Travel::Skip(_, JUMP),
                ..
            } => "Ctrl+D / Ctrl+U",
            Command::Advance {
                travel: Travel::Skip(..),
                ..
            } => "[count]j / [count]k",
            Command::JumpGroupOrEnd(_) => "gg / G",
            Command::GoTo(_) => "[count]G, [count]gg",
            Command::ToggleSlideshow => "a",
            Command::Pan(_) => "H J K L, left drag, arrow keys",
            Command::Zoom {
                factor,
                anchor: Anchor::Centre,
            } if factor == 2.0 || factor == 0.5 => "Ctrl+ / Ctrl-",
            Command::Zoom { .. } => "Right drag, mouse wheel, + / -",
            Command::Fit => "0",
            Command::Rotate(_) => "> / <",
            Command::ToggleMagnificationFilter => "Ctrl+F",
            Command::CycleInfoOverlay => "Ctrl+G",
            Command::Sort(_) => "on, om, os",
            Command::Shuffle => "oz",
            Command::ReverseSort => "or",
            Command::ToggleGrouping => "zg",
            Command::Open { .. } => "e / E",
            Command::DeleteToRecycleBin => "dd, Delete",
            Command::ToggleMarks(_) => "m, [count]m",
            Command::ClearMarks => "M",
            Command::NextMarked(_) => "n / N",
            Command::Transfer(_) => "x / c",
            Command::CopyToClipboard | Command::PasteFromClipboard => "y, Ctrl+C / p, Ctrl+V",
            Command::CopyPathToClipboard => "Y, Ctrl+Shift+C",
            Command::ToggleFullscreen => "f, F11, Enter",
            Command::LeaveFullscreen => "Esc",
            Command::Quit => "q, ZZ",
            Command::TogglePreferences => ":",
            Command::ToggleHelp => "?",
            Command::FocusHelpFilter => "/",
            #[cfg(debug_assertions)]
            Command::TestNotice => "F9",
            Command::JumpGroup(_) | Command::LeaveFullscreenOrQuit => {
                unreachable!("{command:?} is classic only")
            }
        }
    }

    #[test]
    fn caps_lock_does_not_turn_paging_into_panning() {
        let mut vim = Vim::default();
        let caps_j = Key::Character("J".into());
        assert_eq!(
            vim.pressed(&caps_j, ModifiersState::empty(), false),
            Some(advance(Direction::Forward, false))
        );
        assert_eq!(
            vim.pressed(&caps_j, ModifiersState::SHIFT, false),
            Some(pan_key(NamedKey::ArrowDown))
        );
    }

    #[test]
    fn shift_held_between_keys_keeps_what_waits() {
        let mut vim = Vim::default();
        let shift = ModifiersState::SHIFT;
        let z = Key::Character("Z".into());
        assert_eq!(vim.pressed(&z, shift, false), None);
        assert_eq!(
            vim.pressed(&Key::Named(NamedKey::Shift), shift, false),
            None
        );
        assert_eq!(vim.pending().as_deref(), Some("Z"));
        assert_eq!(vim.pressed(&z, shift, false), Some(Command::Quit));
    }

    #[test]
    fn a_key_that_completes_nothing_spends_what_waits() {
        // g then j is not a binding: neither acts, and the next j is a plain j.
        assert_eq!(
            typed("gjj"),
            [None, None, Some(advance(Direction::Forward, false))]
        );
        // A count before a key that takes none is dropped with it.
        assert_eq!(
            typed("5ej"),
            [
                None,
                Some(Command::Open { recursive: false }),
                Some(advance(Direction::Forward, false))
            ]
        );
        assert_eq!(
            typed("zZZ"),
            [None, None, None],
            "zZ is nothing; its Z is spent"
        );
    }

    #[test]
    fn esc_clears_what_waits_before_leaving_fullscreen() {
        assert_eq!(
            typed("5g<Esc>g<Esc><Esc>"),
            [None, None, None, None, None, Some(Command::LeaveFullscreen)]
        );
        let mut vim = Vim::default();
        assert_eq!(
            vim.pressed(&Key::Character("1".into()), ModifiersState::empty(), false),
            None
        );
        assert_eq!(vim.pending().as_deref(), Some("1"));
        assert_eq!(
            vim.pressed(
                &Key::Named(NamedKey::Escape),
                ModifiersState::empty(),
                false
            ),
            None
        );
        assert_eq!(vim.pending(), None);
    }

    #[test]
    fn zero_is_fit_alone_and_a_digit_after_another() {
        assert_eq!(typed("0"), [Some(Command::Fit)]);
        assert_eq!(typed("10G"), [None, None, Some(Command::GoTo(10))]);
        let mut vim = Vim::default();
        for _ in 0..20 {
            vim.pressed(&Key::Character("9".into()), ModifiersState::empty(), false);
        }
        assert_eq!(vim.pending(), Some(MAX_COUNT.to_string()), "no overflow");
    }

    #[test]
    fn held_keys_repeat_moves_and_pans_but_never_sequences() {
        let none = ModifiersState::empty();
        let mut vim = Vim::default();
        let j = Key::Character("j".into());
        assert_eq!(
            vim.pressed(&j, none, true),
            Some(advance(Direction::Forward, true))
        );
        assert_eq!(
            vim.pressed(&Key::Character("d".into()), ModifiersState::CONTROL, true),
            Some(Command::Advance {
                travel: Travel::Skip(Direction::Forward, JUMP),
                repeat: true
            })
        );
        assert!(matches!(
            vim.pressed(&Key::Character("L".into()), ModifiersState::SHIFT, true),
            Some(Command::Pan(_))
        ));
        // A held g makes one g, not gg; a held d deletes nothing.
        let g = Key::Character("g".into());
        assert_eq!(vim.pressed(&g, none, false), None);
        assert_eq!(vim.pressed(&g, none, true), None);
        assert_eq!(vim.pending().as_deref(), Some("g"));
        // While a g waits, a held j does not move either.
        assert_eq!(vim.pressed(&j, none, true), None);
        let mut vim = Vim::default();
        let d = Key::Character("d".into());
        assert_eq!(vim.pressed(&d, none, false), None);
        assert_eq!(vim.pressed(&d, none, true), None);
        // Toggles act once per press.
        let mut vim = Vim::default();
        assert_eq!(vim.pressed(&Key::Character("f".into()), none, true), None);
        assert_eq!(vim.pressed(&Key::Named(NamedKey::Escape), none, true), None);
    }

    #[test]
    fn letting_go_of_a_moving_key_names_its_direction() {
        for (key, direction) in [
            ("j", Direction::Forward),
            ("d", Direction::Forward),
            ("K", Direction::Backward),
            ("u", Direction::Backward),
        ] {
            assert_eq!(
                released(&Key::Character(key.into())),
                Some(direction),
                "{key}"
            );
        }
        assert_eq!(
            released(&Key::Named(NamedKey::Space)),
            Some(Direction::Forward)
        );
        assert_eq!(released(&Key::Character("a".into())), None);
    }

    #[test]
    fn alt_chords_and_lowercase_h_l_do_nothing() {
        let mut vim = Vim::default();
        assert_eq!(
            vim.pressed(&Key::Character("j".into()), ModifiersState::ALT, false),
            None
        );
        assert_eq!(typed("h"), [None]);
        assert_eq!(typed("l"), [None]);
    }

    #[test]
    fn shift_turns_a_pixel_copy_into_a_path_copy() {
        let pixels = Some(Command::CopyToClipboard);
        let path = Some(Command::CopyPathToClipboard);
        assert_eq!(typed("y"), [pixels]);
        assert_eq!(typed("<C-c>"), [pixels]);
        assert_eq!(typed("Y"), [path]);
        assert_eq!(typed("<C-S-c>"), [path]);
        // Caps Lock's Y has no Shift, so it still copies pixels.
        let mut vim = Vim::default();
        let caps_y = Key::Character("Y".into());
        assert_eq!(vim.pressed(&caps_y, ModifiersState::empty(), false), pixels);
    }

    #[test]
    fn the_help_overlay_scrolls_with_vim_keys() {
        let none = ModifiersState::empty();
        let mut vim = Vim::default();
        let mut help = |key: Key, modifiers| vim.help_key(&key, modifiers, false);
        let char = |c: &str| Key::Character(c.into());
        assert_eq!(
            help(char("j"), none),
            Some(HelpKey::Scroll(Scroll::Lines(1.0)))
        );
        assert_eq!(
            help(char("u"), ModifiersState::CONTROL),
            Some(HelpKey::Scroll(Scroll::Pages(-0.5)))
        );
        assert_eq!(
            help(char("G"), ModifiersState::SHIFT),
            Some(HelpKey::Scroll(Scroll::Bottom))
        );
        assert_eq!(help(char("g"), none), None);
        assert_eq!(help(char("g"), none), Some(HelpKey::Scroll(Scroll::Top)));
        // Esc with a g waiting clears the g; the next closes.
        assert_eq!(help(char("g"), none), None);
        assert_eq!(help(Key::Named(NamedKey::Escape), none), None);
        assert_eq!(
            help(Key::Named(NamedKey::Escape), none),
            Some(HelpKey::Close)
        );
        assert_eq!(help(char("?"), none), Some(HelpKey::Close));
        assert_eq!(help(char("/"), none), Some(HelpKey::FocusFilter));
        // Bindings underneath do nothing.
        assert_eq!(help(char("d"), none), None);
        assert_eq!(help(char("d"), none), None);
        assert_eq!(help(Key::Named(NamedKey::Space), none), None);
        assert_eq!(
            vim.help_key(&Key::Character("j".into()), none, true),
            Some(HelpKey::Scroll(Scroll::Lines(1.0))),
            "held j keeps scrolling"
        );
    }

    #[test]
    fn the_preferences_screen_closes_on_colon_and_esc_only() {
        let none = ModifiersState::empty();
        assert!(closes_preferences(
            &Key::Character(":".into()),
            ModifiersState::SHIFT,
            false
        ));
        assert!(closes_preferences(
            &Key::Named(NamedKey::Escape),
            none,
            false
        ));
        assert!(!closes_preferences(
            &Key::Named(NamedKey::Escape),
            none,
            true
        ));
        assert!(!closes_preferences(
            &Key::Character("p".into()),
            none,
            false
        ));
        assert!(!closes_preferences(
            &Key::Character("q".into()),
            none,
            false
        ));
    }
}
