//! The classic keymap: the original SuperSlide's keys, one press per binding.

use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::{
    CLICK_NEXT, CLICK_PREVIOUS, DRAG_PAN, DRAG_ZOOM, HelpKey, Hints, Input, Probe, Row, WHEEL_ZOOM,
    ZOOM_STEP, advance, help_scroll, pan_key, row, zoom,
};
use crate::browse::domain::{Direction, SortKey};
use crate::file_actions::domain::Transfer;
use crate::help::domain::Section;
use crate::prefs::presentation::PreferenceKeys;
use crate::shell::command::Command;
use crate::view::domain::Turn;
use crate::welcome::presentation::OpeningKeys;

pub const HINTS: Hints = Hints {
    openings: OpeningKeys {
        file: "O",
        folder: "Shift+O",
        help: "?",
    },
    help: "Wheel, arrows, PgUp / PgDn, Home / End scroll · Esc closes",
    preferences: PreferenceKeys {
        close: "P or Esc",
        advance: "Held Space or Backspace",
        magnification: "Ctrl+F",
        info: "I",
    },
};

pub fn for_key(key: &Key, modifiers: ModifiersState, repeat: bool) -> Option<Command> {
    let ctrl = modifiers.control_key();
    let plain = !ctrl && !modifiers.alt_key();
    let (command, repeats) = match key {
        Key::Named(NamedKey::F11 | NamedKey::Enter) => (Command::ToggleFullscreen, false),
        Key::Named(NamedKey::Escape) => (Command::LeaveFullscreenOrQuit, false),
        // Repeat passes through: the advance limit, not the keyboard, sets the pace.
        Key::Named(NamedKey::Space) if plain => (advance(Direction::Forward, repeat), true),
        Key::Named(NamedKey::Backspace) if plain => (advance(Direction::Backward, repeat), true),
        Key::Named(NamedKey::ArrowLeft) if ctrl && !modifiers.alt_key() => {
            (Command::JumpGroup(Direction::Backward), false)
        }
        Key::Named(NamedKey::ArrowRight) if ctrl && !modifiers.alt_key() => {
            (Command::JumpGroup(Direction::Forward), false)
        }
        Key::Named(NamedKey::Delete) if plain => (Command::DeleteToRecycleBin, false),
        #[cfg(debug_assertions)]
        Key::Named(NamedKey::F9) => (Command::TestNotice, false),
        Key::Named(
            arrow @ (NamedKey::ArrowLeft
            | NamedKey::ArrowRight
            | NamedKey::ArrowUp
            | NamedKey::ArrowDown),
        ) if plain => (pan_key(*arrow), true),
        Key::Character(c) => {
            // Shift changes the character ("=" to "+", "r" to "R"), so it is not checked; the
            // unshifted "=" counts as + so zooming in needs no Shift.
            let factor = if ctrl { 2.0 } else { ZOOM_STEP };
            match c.as_str() {
                "+" | "=" => (zoom(factor), true),
                "-" => (zoom(1.0 / factor), true),
                "f" | "F" if ctrl && !modifiers.alt_key() => {
                    (Command::ToggleMagnificationFilter, false)
                }
                "c" | "C" if ctrl && !modifiers.alt_key() => (Command::CopyToClipboard, false),
                "v" | "V" if ctrl && !modifiers.alt_key() => (Command::PasteFromClipboard, false),
                _ if !plain => return None,
                "0" => (Command::Fit, false),
                "r" | "R" => (Command::Rotate(Turn::Right), false),
                "l" | "L" => (Command::Rotate(Turn::Left), false),
                "n" | "N" => (Command::Sort(SortKey::Name), false),
                "d" | "D" => (Command::Sort(SortKey::Date), false),
                "z" | "Z" => (Command::Sort(SortKey::Size), false),
                "s" | "S" => (Command::Shuffle, false),
                "v" | "V" => (Command::ReverseSort, false),
                "g" | "G" => (Command::ToggleGrouping, false),
                "a" | "A" => (Command::ToggleSlideshow, false),
                "i" | "I" => (Command::CycleInfoOverlay, false),
                "p" | "P" => (Command::TogglePreferences, false),
                "?" => (Command::ToggleHelp, false),
                "/" => (Command::FocusHelpFilter, false),
                "m" | "M" => (Command::Transfer(Transfer::Move), false),
                "c" | "C" => (Command::Transfer(Transfer::Copy), false),
                // The Shift key, not the character, so Caps Lock does not turn O recursive.
                "o" | "O" => (
                    Command::Open {
                        recursive: modifiers.shift_key(),
                    },
                    false,
                ),
                _ => return None,
            }
        }
        _ => return None,
    };
    // Held toggles would flicker between states, and a held Esc would leave fullscreen then quit.
    (repeats || !repeat).then_some(command)
}

pub fn for_help_key(key: &Key, modifiers: ModifiersState, repeat: bool) -> Option<HelpKey> {
    if let Some(scroll) = help_scroll(key) {
        return Some(HelpKey::Scroll(scroll));
    }
    match key {
        // Esc closes rather than leaving fullscreen or quitting; ? and / keep their bindings.
        Key::Named(NamedKey::Escape) => (!repeat).then_some(HelpKey::Close),
        _ => match for_key(key, modifiers, repeat)? {
            Command::ToggleHelp => Some(HelpKey::Close),
            Command::FocusHelpFilter => Some(HelpKey::FocusFilter),
            _ => None,
        },
    }
}

/// P and Esc close the preferences screen.
pub fn closes_preferences(key: &Key, modifiers: ModifiersState, repeat: bool) -> bool {
    match key {
        // Esc closes rather than leaving fullscreen or quitting.
        Key::Named(NamedKey::Escape) => !repeat,
        _ => for_key(key, modifiers, repeat) == Some(Command::TogglePreferences),
    }
}

pub fn released(key: &Key) -> Option<Direction> {
    match key {
        Key::Named(NamedKey::Space) => Some(Direction::Forward),
        Key::Named(NamedKey::Backspace) => Some(Direction::Backward),
        _ => None,
    }
}

const NONE: ModifiersState = ModifiersState::empty();
const CTRL: ModifiersState = ModifiersState::CONTROL;

const fn named(key: NamedKey, modifiers: ModifiersState, command: Command) -> Probe {
    Probe(Input::Named(key, modifiers), command)
}

const fn char(c: &'static str, modifiers: ModifiersState, command: Command) -> Probe {
    Probe(Input::Char(c, modifiers), command)
}

/// Every binding, in the order of `docs/context/keymap.md`'s classic table, each section in one
/// run.
pub const BINDINGS: &[Row] = &[
    row(
        "Space, right click",
        "Next image",
        Section::Navigate,
        &[
            named(NamedKey::Space, NONE, advance(Direction::Forward, false)),
            CLICK_NEXT,
        ],
    ),
    row(
        "Backspace, left click",
        "Previous image",
        Section::Navigate,
        &[
            named(
                NamedKey::Backspace,
                NONE,
                advance(Direction::Backward, false),
            ),
            CLICK_PREVIOUS,
        ],
    ),
    row(
        "Ctrl+Right / Ctrl+Left",
        "First image of the next / previous group (group by subfolder only)",
        Section::Navigate,
        &[
            named(
                NamedKey::ArrowRight,
                CTRL,
                Command::JumpGroup(Direction::Forward),
            ),
            named(
                NamedKey::ArrowLeft,
                CTRL,
                Command::JumpGroup(Direction::Backward),
            ),
        ],
    ),
    row(
        "A",
        "Start or stop the slideshow",
        Section::Navigate,
        &[char("a", NONE, Command::ToggleSlideshow)],
    ),
    row(
        "Left drag, arrow keys",
        "Pan",
        Section::View,
        &[
            DRAG_PAN,
            named(NamedKey::ArrowLeft, NONE, pan_key(NamedKey::ArrowLeft)),
            named(NamedKey::ArrowRight, NONE, pan_key(NamedKey::ArrowRight)),
            named(NamedKey::ArrowUp, NONE, pan_key(NamedKey::ArrowUp)),
            named(NamedKey::ArrowDown, NONE, pan_key(NamedKey::ArrowDown)),
        ],
    ),
    row(
        "Right drag, mouse wheel, + / -",
        "Zoom (+ / - and each wheel notch: 1.25×; = also zooms in)",
        Section::View,
        &[
            DRAG_ZOOM,
            WHEEL_ZOOM,
            char("+", NONE, zoom(ZOOM_STEP)),
            char("=", NONE, zoom(ZOOM_STEP)),
            char("-", NONE, zoom(1.0 / ZOOM_STEP)),
        ],
    ),
    row(
        "Ctrl+ / Ctrl-",
        "Zoom by double / half",
        Section::View,
        &[
            char("+", CTRL, zoom(2.0)),
            char("=", CTRL, zoom(2.0)),
            char("-", CTRL, zoom(0.5)),
        ],
    ),
    row(
        "0",
        "Back to fit",
        Section::View,
        &[char("0", NONE, Command::Fit)],
    ),
    row(
        "R / L",
        "Screen rotation right / left",
        Section::View,
        &[
            char("r", NONE, Command::Rotate(Turn::Right)),
            char("l", NONE, Command::Rotate(Turn::Left)),
        ],
    ),
    row(
        "Ctrl+F",
        "Toggle the magnification filter between linear and nearest",
        Section::View,
        &[char("f", CTRL, Command::ToggleMagnificationFilter)],
    ),
    row(
        "I",
        "Cycle the info overlay",
        Section::View,
        &[char("i", NONE, Command::CycleInfoOverlay)],
    ),
    row(
        "N, D, Z",
        "Sort by name, date, size",
        Section::Order,
        &[
            char("n", NONE, Command::Sort(SortKey::Name)),
            char("d", NONE, Command::Sort(SortKey::Date)),
            char("z", NONE, Command::Sort(SortKey::Size)),
        ],
    ),
    row(
        "S",
        "Shuffle (again, if already shuffled)",
        Section::Order,
        &[char("s", NONE, Command::Shuffle)],
    ),
    row(
        "V",
        "Reverse the sort",
        Section::Order,
        &[char("v", NONE, Command::ReverseSort)],
    ),
    row(
        "G",
        "Toggle group by subfolder",
        Section::Order,
        &[char("g", NONE, Command::ToggleGrouping)],
    ),
    row(
        "O / Shift+O",
        "Open a file / recursive open of a folder",
        Section::Files,
        &[
            char("o", NONE, Command::Open { recursive: false }),
            char(
                "O",
                ModifiersState::SHIFT,
                Command::Open { recursive: true },
            ),
        ],
    ),
    row(
        "Delete",
        "Delete the current image to the Recycle Bin",
        Section::Files,
        &[named(NamedKey::Delete, NONE, Command::DeleteToRecycleBin)],
    ),
    row(
        "M / C",
        "Move / copy the current image to a folder (picker opens at the last target folder)",
        Section::Files,
        &[
            char("m", NONE, Command::Transfer(Transfer::Move)),
            char("c", NONE, Command::Transfer(Transfer::Copy)),
        ],
    ),
    row(
        "Ctrl+C / Ctrl+V",
        "Copy the current image to the clipboard / show the clipboard image",
        Section::Clipboard,
        &[
            char("c", CTRL, Command::CopyToClipboard),
            char("v", CTRL, Command::PasteFromClipboard),
        ],
    ),
    row(
        "F11, Enter",
        "Toggle fullscreen",
        Section::Window,
        &[
            named(NamedKey::F11, NONE, Command::ToggleFullscreen),
            named(NamedKey::Enter, NONE, Command::ToggleFullscreen),
        ],
    ),
    row(
        "Esc",
        "Leave fullscreen; in a window, quit",
        Section::Window,
        &[named(
            NamedKey::Escape,
            NONE,
            Command::LeaveFullscreenOrQuit,
        )],
    ),
    row(
        "P",
        "Open or close preferences",
        Section::Window,
        &[char("p", NONE, Command::TogglePreferences)],
    ),
    row(
        "?",
        "Show or hide this list of keys",
        Section::Window,
        &[char("?", ModifiersState::SHIFT, Command::ToggleHelp)],
    ),
    row(
        "/",
        "Filter this list of keys",
        Section::Window,
        &[char("/", NONE, Command::FocusHelpFilter)],
    ),
    // Last, so the rows above stay the doc's table in every build.
    #[cfg(debug_assertions)]
    row(
        "F9",
        "Show a test notice",
        Section::Window,
        &[named(NamedKey::F9, NONE, Command::TestNotice)],
    ),
];

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::browse::domain::Travel;
    use crate::help::domain::Scroll;
    use crate::shell::keymap::tests::replay_mouse;
    use crate::view::domain::Anchor;

    pub fn replay(input: &Input) -> Option<Command> {
        if let Some(command) = replay_mouse(input) {
            return command;
        }
        match *input {
            Input::Named(key, modifiers) => for_key(&Key::Named(key), modifiers, false),
            Input::Char(c, modifiers) => for_key(&Key::Character(c.into()), modifiers, false),
            _ => panic!("the classic keymap has no Vim key probes"),
        }
    }

    /// The row each command belongs to. Exhaustive on purpose: a new command does not compile
    /// until it names its row, and the probe test then needs a probe there that gives it.
    pub fn row_of(command: Command) -> &'static str {
        match command {
            Command::Advance {
                travel: Travel::Step(Direction::Forward),
                ..
            } => "Space, right click",
            Command::Advance {
                travel: Travel::Step(Direction::Backward),
                ..
            } => "Backspace, left click",
            Command::JumpGroup(_) => "Ctrl+Right / Ctrl+Left",
            Command::ToggleSlideshow => "A",
            Command::Pan(_) => "Left drag, arrow keys",
            Command::Zoom {
                factor,
                anchor: Anchor::Centre,
            } if factor == 2.0 || factor == 0.5 => "Ctrl+ / Ctrl-",
            Command::Zoom { .. } => "Right drag, mouse wheel, + / -",
            Command::Fit => "0",
            Command::Rotate(_) => "R / L",
            Command::ToggleMagnificationFilter => "Ctrl+F",
            Command::CycleInfoOverlay => "I",
            Command::ToggleFullscreen => "F11, Enter",
            Command::LeaveFullscreenOrQuit => "Esc",
            Command::Sort(_) => "N, D, Z",
            Command::Shuffle => "S",
            Command::ReverseSort => "V",
            Command::ToggleGrouping => "G",
            Command::Open { .. } => "O / Shift+O",
            Command::DeleteToRecycleBin => "Delete",
            Command::Transfer(_) => "M / C",
            Command::CopyToClipboard | Command::PasteFromClipboard => "Ctrl+C / Ctrl+V",
            Command::TogglePreferences => "P",
            Command::ToggleHelp => "?",
            Command::FocusHelpFilter => "/",
            #[cfg(debug_assertions)]
            Command::TestNotice => "F9",
            Command::Advance {
                travel: Travel::Skip(..),
                ..
            }
            | Command::JumpGroupOrEnd(_)
            | Command::GoTo(_)
            | Command::ToggleMarks(_)
            | Command::ClearMarks
            | Command::NextMarked(_)
            | Command::LeaveFullscreen
            | Command::Quit => unreachable!("{command:?} is Vim only"),
        }
    }

    #[test]
    fn the_help_overlay_takes_every_key() {
        let none = ModifiersState::empty();
        let named = |key| for_help_key(&Key::Named(key), none, false);
        let char = |c: &str| for_help_key(&Key::Character(c.into()), none, false);
        assert_eq!(named(NamedKey::Escape), Some(HelpKey::Close));
        assert_eq!(char("?"), Some(HelpKey::Close));
        assert_eq!(char("/"), Some(HelpKey::FocusFilter));
        assert_eq!(named(NamedKey::End), Some(HelpKey::Scroll(Scroll::Bottom)));
        assert_eq!(
            for_help_key(&Key::Named(NamedKey::ArrowDown), none, true),
            Some(HelpKey::Scroll(Scroll::Lines(1.0))),
            "held arrows keep scrolling"
        );
        // Bindings underneath do nothing: no rotate, delete, advance or fullscreen.
        for key in [
            NamedKey::Delete,
            NamedKey::Space,
            NamedKey::F11,
            NamedKey::Enter,
        ] {
            assert_eq!(named(key), None, "{key:?}");
        }
        assert_eq!(char("r"), None);
        // A held Esc closes once; the repeat must not reach anything else.
        assert_eq!(
            for_help_key(&Key::Named(NamedKey::Escape), none, true),
            None
        );
    }

    #[test]
    fn the_preferences_screen_closes_on_p_and_esc_only() {
        let none = ModifiersState::empty();
        let closes = |key: Key, repeat| closes_preferences(&key, none, repeat);
        assert!(closes(Key::Character("p".into()), false));
        assert!(closes(Key::Character("P".into()), false));
        assert!(closes(Key::Named(NamedKey::Escape), false));
        // A held Esc closes once, and its repeats must not quit underneath.
        assert!(!closes(Key::Named(NamedKey::Escape), true));
        for key in [NamedKey::Space, NamedKey::Delete, NamedKey::F11] {
            assert!(!closes(Key::Named(key), false), "{key:?}");
        }
        assert!(!closes(Key::Character("?".into()), false));
        assert!(!closes_preferences(
            &Key::Character("p".into()),
            ModifiersState::CONTROL,
            false
        ));
    }

    #[test]
    fn toggles_ignore_key_repeat_but_pan_and_zoom_repeat() {
        let none = ModifiersState::empty();
        let f11 = Key::Named(NamedKey::F11);
        assert_eq!(for_key(&f11, none, false), Some(Command::ToggleFullscreen));
        assert_eq!(for_key(&f11, none, true), None);
        // A held A would start and stop the slideshow thirty times a second.
        let a = Key::Character("a".into());
        assert_eq!(for_key(&a, none, false), Some(Command::ToggleSlideshow));
        assert_eq!(for_key(&a, none, true), None);
        // A held Delete must not empty the folder.
        let delete = Key::Named(NamedKey::Delete);
        assert_eq!(
            for_key(&delete, none, false),
            Some(Command::DeleteToRecycleBin)
        );
        assert_eq!(for_key(&delete, none, true), None);
        let right = Key::Named(NamedKey::ArrowRight);
        assert!(matches!(for_key(&right, none, true), Some(Command::Pan(_))));
        let plus = Key::Character("+".into());
        assert!(matches!(
            for_key(&plus, none, true),
            Some(Command::Zoom { .. })
        ));
    }

    #[test]
    fn ctrl_changes_the_zoom_step_and_frees_letters() {
        let ctrl = ModifiersState::CONTROL;
        let Some(Command::Zoom { factor, .. }) = for_key(&Key::Character("=".into()), ctrl, false)
        else {
            panic!("Ctrl+ did not zoom");
        };
        assert_eq!(factor, 2.0);
        assert_eq!(
            for_key(&Key::Character("f".into()), ctrl, false),
            Some(Command::ToggleMagnificationFilter)
        );
        assert_eq!(
            for_key(&Key::Character("f".into()), ModifiersState::empty(), false),
            None
        );
        // Ctrl+R and Ctrl+arrows belong to other bindings, not rotate and pan; Ctrl+C is the
        // clipboard, not a copy to a folder.
        assert_eq!(for_key(&Key::Character("r".into()), ctrl, false), None);
        assert_eq!(
            for_key(&Key::Character("c".into()), ctrl, false),
            Some(Command::CopyToClipboard)
        );
        assert_eq!(
            for_key(&Key::Character("v".into()), ctrl, false),
            Some(Command::PasteFromClipboard)
        );
        // A held Ctrl+V pastes once, not once per key repeat.
        assert_eq!(for_key(&Key::Character("v".into()), ctrl, true), None);
        assert_eq!(
            for_key(&Key::Named(NamedKey::ArrowLeft), ctrl, false),
            Some(Command::JumpGroup(Direction::Backward))
        );
        assert_eq!(for_key(&Key::Named(NamedKey::ArrowLeft), ctrl, true), None);
    }

    #[test]
    fn held_space_repeats_for_the_advance_limit_to_pace() {
        let none = ModifiersState::empty();
        let space = Key::Named(NamedKey::Space);
        assert_eq!(
            for_key(&space, none, true),
            Some(advance(Direction::Forward, true))
        );
        assert_eq!(released(&space), Some(Direction::Forward));
        // Ctrl+Space is not a navigation key, but letting go of it still is.
        assert_eq!(for_key(&space, ModifiersState::CONTROL, false), None);
    }

    #[test]
    fn shift_not_caps_lock_makes_o_recursive() {
        let caps_o = Key::Character("O".into());
        assert_eq!(
            for_key(&caps_o, ModifiersState::empty(), false),
            Some(Command::Open { recursive: false })
        );
        assert_eq!(
            for_key(&caps_o, ModifiersState::SHIFT, false),
            Some(Command::Open { recursive: true })
        );
    }
}
