//! Maps keys and mouse gestures to [`Command`]. The only place bindings are decided: `for_key`,
//! `for_wheel` and `Mouse` decide them, and [`BINDINGS`] describes them for the help overlay. Tests
//! hold those two and `docs/context/keymap.md` together, so none can change alone.

use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::browse::domain::{Direction, SortKey};
use crate::file_actions::domain::Transfer;
use crate::help::domain::{Binding, Scroll, Section};
use crate::shell::command::Command;
use crate::view::domain::{Anchor, PanBy, Turn};

/// Zoom per wheel notch and per + / - press.
const ZOOM_STEP: f32 = 1.25;
/// Share of the window one arrow press pans.
const ARROW_PAN: f32 = 0.1;
/// Movement while a button is down, in window pixels, that still counts as a click. Hands
/// jitter; a click must not nudge the image.
const CLICK_SLOP: f32 = 4.0;
/// Upward right-drag distance, in window pixels, that doubles the zoom.
const DRAG_PX_PER_DOUBLING: f32 = 200.0;
/// Touchpads report pixels, not notches; this many count as one notch.
const PIXELS_PER_NOTCH: f32 = 50.0;

/// The command a key press runs, if any. `repeat` is true for auto-repeat while the key is held.
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
        ) if plain => {
            // Arrows scroll the view: Right shows more of the right side, so the image moves left.
            let (dx, dy) = match arrow {
                NamedKey::ArrowLeft => (ARROW_PAN, 0.0),
                NamedKey::ArrowRight => (-ARROW_PAN, 0.0),
                NamedKey::ArrowUp => (0.0, ARROW_PAN),
                _ => (0.0, -ARROW_PAN),
            };
            (Command::Pan(PanBy::WindowFraction { dx, dy }), true)
        }
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

/// What a key press does while the help overlay is open and its filter box is not focused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HelpKey {
    Close,
    FocusFilter,
    Scroll(Scroll),
}

/// While the help overlay is open it takes every key: the ones below act on it, and every other
/// key does nothing, so a key pressed at the list never runs its binding underneath.
pub fn for_help_key(key: &Key, modifiers: ModifiersState, repeat: bool) -> Option<HelpKey> {
    let scroll = match key {
        Key::Named(NamedKey::ArrowUp) => Scroll::Lines(-1.0),
        Key::Named(NamedKey::ArrowDown) => Scroll::Lines(1.0),
        Key::Named(NamedKey::PageUp) => Scroll::Pages(-1.0),
        Key::Named(NamedKey::PageDown) => Scroll::Pages(1.0),
        Key::Named(NamedKey::Home) => Scroll::Top,
        Key::Named(NamedKey::End) => Scroll::Bottom,
        // Esc closes rather than leaving fullscreen or quitting; ? and / keep their bindings.
        Key::Named(NamedKey::Escape) => return (!repeat).then_some(HelpKey::Close),
        _ => {
            return match for_key(key, modifiers, repeat)? {
                Command::ToggleHelp => Some(HelpKey::Close),
                Command::FocusHelpFilter => Some(HelpKey::FocusFilter),
                _ => None,
            };
        }
    };
    Some(HelpKey::Scroll(scroll))
}

/// Lines the help overlay scrolls per wheel notch.
const HELP_LINES_PER_NOTCH: f32 = 3.0;

/// The wheel scrolls the help overlay, wherever the cursor is, instead of zooming.
pub fn for_help_wheel(delta: MouseScrollDelta) -> Option<Scroll> {
    let notches = notches(delta);
    // Wheel up gives positive notches and scrolls towards the top.
    (notches != 0.0).then(|| Scroll::Lines(-notches * HELP_LINES_PER_NOTCH))
}

/// Space or Backspace let go, whatever the modifiers: a repeat waiting under the advance limit
/// must not advance after it.
pub fn released(key: &Key) -> Option<Direction> {
    match key {
        Key::Named(NamedKey::Space) => Some(Direction::Forward),
        Key::Named(NamedKey::Backspace) => Some(Direction::Backward),
        _ => None,
    }
}

const fn advance(direction: Direction, repeat: bool) -> Command {
    Command::Advance { direction, repeat }
}

const fn zoom(factor: f32) -> Command {
    Command::Zoom {
        factor,
        anchor: Anchor::Centre,
    }
}

/// Wheel up zooms in, about the cursor.
pub fn for_wheel(delta: MouseScrollDelta, cursor: [f32; 2]) -> Option<Command> {
    let notches = notches(delta);
    (notches != 0.0).then(|| Command::Zoom {
        factor: ZOOM_STEP.powf(notches),
        anchor: Anchor::Cursor {
            x: cursor[0],
            y: cursor[1],
        },
    })
}

fn notches(delta: MouseScrollDelta) -> f32 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        MouseScrollDelta::PixelDelta(p) => p.y as f32 / PIXELS_PER_NOTCH,
    }
}

/// Tells clicks from drags. A press becomes a drag once the cursor strays more than
/// `CLICK_SLOP` from where it went down; until then nothing moves, and a release is a click.
#[derive(Debug, Default)]
pub struct Mouse {
    held: Option<Held>,
}

#[derive(Debug)]
struct Held {
    button: MouseButton,
    down_at: [f32; 2],
    last: [f32; 2],
    dragging: bool,
}

impl Mouse {
    /// A second button pressed while one is held is ignored, so each gesture has one meaning.
    pub fn pressed(&mut self, button: MouseButton, at: [f32; 2]) {
        if self.held.is_none() && matches!(button, MouseButton::Left | MouseButton::Right) {
            self.held = Some(Held {
                button,
                down_at: at,
                last: at,
                dragging: false,
            });
        }
    }

    pub fn moved(&mut self, at: [f32; 2]) -> Option<Command> {
        let held = self.held.as_mut()?;
        if !held.dragging {
            let [dx, dy] = [at[0] - held.down_at[0], at[1] - held.down_at[1]];
            if dx.hypot(dy) <= CLICK_SLOP {
                return None;
            }
            // The first drag step covers the slop too, so the image stays under the cursor.
            held.dragging = true;
        }
        let [dx, dy] = [at[0] - held.last[0], at[1] - held.last[1]];
        held.last = at;
        Some(match held.button {
            MouseButton::Left => Command::Pan(PanBy::Pixels { dx, dy }),
            // Up zooms in, about the point where the drag started.
            _ => Command::Zoom {
                factor: 2f32.powf(-dy / DRAG_PX_PER_DOUBLING),
                anchor: Anchor::Cursor {
                    x: held.down_at[0],
                    y: held.down_at[1],
                },
            },
        })
    }

    pub fn released(&mut self, button: MouseButton) -> Option<Command> {
        let held = self.held.take_if(|held| held.button == button)?;
        if held.dragging {
            return None;
        }
        Some(match button {
            MouseButton::Left => advance(Direction::Backward, false),
            _ => advance(Direction::Forward, false),
        })
    }
}

/// One row of the keymap: the binding as the help overlay shows it, and sample inputs with the
/// command each must give.
pub struct Row {
    pub binding: Binding,
    /// Run through `for_key`, `for_wheel` and `Mouse` by the tests. Empty only for a row whose
    /// command is not built yet.
    probes: &'static [Probe],
}

impl Row {
    /// Whether its keys do anything yet. The help overlay lists only rows that do.
    pub fn works(&self) -> bool {
        !self.probes.is_empty()
    }
}

/// An input a test can replay, with the command it must give.
#[cfg_attr(not(test), expect(dead_code, reason = "only tests replay probes"))]
struct Probe(Input, Command);

#[cfg_attr(not(test), expect(dead_code, reason = "only tests replay probes"))]
enum Input {
    Named(NamedKey, ModifiersState),
    Char(&'static str, ModifiersState),
    /// Press and release at `PROBE_AT` without moving.
    Click(MouseButton),
    /// Press at `PROBE_AT`, then move by the offset.
    Drag(MouseButton, [f32; 2]),
    /// Wheel notches with the cursor at `PROBE_AT`.
    Wheel(f32),
}

/// Where mouse probes happen, so the commands they expect can name their anchor.
const PROBE_AT: [f32; 2] = [100.0, 100.0];
const NONE: ModifiersState = ModifiersState::empty();
const CTRL: ModifiersState = ModifiersState::CONTROL;

const fn named(key: NamedKey, modifiers: ModifiersState, command: Command) -> Probe {
    Probe(Input::Named(key, modifiers), command)
}

const fn char(c: &'static str, modifiers: ModifiersState, command: Command) -> Probe {
    Probe(Input::Char(c, modifiers), command)
}

const fn row(
    keys: &'static str,
    action: &'static str,
    section: Section,
    probes: &'static [Probe],
) -> Row {
    Row {
        binding: Binding {
            keys,
            action,
            section,
        },
        probes,
    }
}

const fn pan_by(dx: f32, dy: f32) -> Command {
    Command::Pan(PanBy::WindowFraction { dx, dy })
}

const fn zoom_at_probe(factor: f32) -> Command {
    Command::Zoom {
        factor,
        anchor: Anchor::Cursor {
            x: PROBE_AT[0],
            y: PROBE_AT[1],
        },
    }
}

/// Every binding, in the order of `docs/context/keymap.md`'s table, each section in one run.
pub const BINDINGS: &[Row] = &[
    row(
        "Space, right click",
        "Next image",
        Section::Navigate,
        &[
            named(NamedKey::Space, NONE, advance(Direction::Forward, false)),
            Probe(
                Input::Click(MouseButton::Right),
                advance(Direction::Forward, false),
            ),
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
            Probe(
                Input::Click(MouseButton::Left),
                advance(Direction::Backward, false),
            ),
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
            Probe(
                Input::Drag(MouseButton::Left, [10.0, 0.0]),
                Command::Pan(PanBy::Pixels { dx: 10.0, dy: 0.0 }),
            ),
            named(NamedKey::ArrowLeft, NONE, pan_by(ARROW_PAN, 0.0)),
            named(NamedKey::ArrowRight, NONE, pan_by(-ARROW_PAN, 0.0)),
            named(NamedKey::ArrowUp, NONE, pan_by(0.0, ARROW_PAN)),
            named(NamedKey::ArrowDown, NONE, pan_by(0.0, -ARROW_PAN)),
        ],
    ),
    row(
        "Right drag, mouse wheel, + / -",
        "Zoom (+ / - and each wheel notch: 1.25×; = also zooms in)",
        Section::View,
        &[
            Probe(
                Input::Drag(MouseButton::Right, [0.0, -DRAG_PX_PER_DOUBLING]),
                zoom_at_probe(2.0),
            ),
            Probe(Input::Wheel(1.0), zoom_at_probe(ZOOM_STEP)),
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
    row("I", "Cycle the info overlay", Section::View, &[]),
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
    row("P", "Open or close preferences", Section::Window, &[]),
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
mod tests {
    use super::*;

    fn replay(input: &Input) -> Option<Command> {
        let mut mouse = Mouse::default();
        match *input {
            Input::Named(key, modifiers) => for_key(&Key::Named(key), modifiers, false),
            Input::Char(c, modifiers) => for_key(&Key::Character(c.into()), modifiers, false),
            Input::Click(button) => {
                mouse.pressed(button, PROBE_AT);
                mouse.released(button)
            }
            Input::Drag(button, [dx, dy]) => {
                mouse.pressed(button, PROBE_AT);
                mouse.moved([PROBE_AT[0] + dx, PROBE_AT[1] + dy])
            }
            Input::Wheel(notches) => for_wheel(MouseScrollDelta::LineDelta(0.0, notches), PROBE_AT),
        }
    }

    /// The row each command belongs to. Exhaustive on purpose: a new command does not compile
    /// until it names its row, and the probe test then needs a probe there that gives it.
    fn row_of(command: Command) -> &'static str {
        match command {
            Command::Advance {
                direction: Direction::Forward,
                ..
            } => "Space, right click",
            Command::Advance {
                direction: Direction::Backward,
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
        }
    }

    #[test]
    fn every_probe_gives_its_command_on_its_row() {
        for row in BINDINGS {
            let keys = row.binding.keys;
            for Probe(input, command) in row.probes {
                assert_eq!(replay(input), Some(*command), "probe on row {keys}");
                assert_eq!(
                    row_of(*command),
                    keys,
                    "{command:?} probed on the wrong row"
                );
            }
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
        assert_eq!(
            for_help_wheel(MouseScrollDelta::LineDelta(0.0, -1.0)),
            Some(Scroll::Lines(3.0)),
            "wheel down scrolls down"
        );
    }

    #[test]
    fn only_unbuilt_commands_lack_probes() {
        let unprobed: Vec<_> = BINDINGS
            .iter()
            .filter(|row| row.probes.is_empty())
            .map(|row| row.binding.keys)
            .collect();
        // The info overlay (SSR-17) and preferences (SSR-18); each issue adds its probe.
        assert_eq!(unprobed, ["I", "P"]);
    }

    #[test]
    fn the_keymap_doc_lists_the_same_rows() {
        let doc = include_str!("../../docs/context/keymap.md");
        let table: Vec<(&str, &str)> = doc
            .lines()
            .skip_while(|line| *line != "| Input | Command |")
            .skip(2)
            .take_while(|line| line.starts_with('|'))
            .map(|line| {
                let (keys, action) = line
                    .trim_matches('|')
                    .split_once(" | ")
                    .expect("a two-column row");
                (keys.trim(), action.trim())
            })
            .collect();
        assert!(!table.is_empty(), "keymap.md has no binding table");
        let rows: Vec<_> = BINDINGS
            .iter()
            .map(|row| (row.binding.keys, row.binding.action))
            .collect();
        assert_eq!(table, rows[..table.len()]);
        // The doc mentions F9 in prose, as release builds lack it.
        let extra: Vec<_> = rows[table.len()..].iter().map(|row| row.0).collect();
        let expected: &[&str] = if cfg!(debug_assertions) { &["F9"] } else { &[] };
        assert_eq!(extra, expected);
    }

    #[test]
    fn each_section_is_one_run() {
        let mut seen = Vec::new();
        for row in BINDINGS {
            let section = row.binding.section;
            if seen.last() != Some(&section) {
                assert!(!seen.contains(&section), "{section:?} is split");
                seen.push(section);
            }
        }
    }

    #[test]
    fn a_click_with_jitter_does_not_pan() {
        let mut mouse = Mouse::default();
        mouse.pressed(MouseButton::Left, [100.0, 100.0]);
        for at in [[102.0, 101.0], [97.0, 98.0], [104.0, 100.0], [100.0, 96.0]] {
            assert_eq!(mouse.moved(at), None, "jitter to {at:?} panned");
        }
        assert_eq!(
            mouse.released(MouseButton::Left),
            Some(advance(Direction::Backward, false))
        );

        mouse.pressed(MouseButton::Right, [100.0, 100.0]);
        assert_eq!(mouse.moved([103.0, 102.0]), None);
        assert_eq!(
            mouse.released(MouseButton::Right),
            Some(advance(Direction::Forward, false))
        );
    }

    #[test]
    fn a_left_drag_pans_by_the_whole_distance_and_is_not_a_click() {
        let mut mouse = Mouse::default();
        mouse.pressed(MouseButton::Left, [100.0, 100.0]);
        assert_eq!(
            mouse.moved([110.0, 100.0]),
            Some(Command::Pan(PanBy::Pixels { dx: 10.0, dy: 0.0 }))
        );
        // Once dragging, small steps pan too.
        assert_eq!(
            mouse.moved([111.0, 99.0]),
            Some(Command::Pan(PanBy::Pixels { dx: 1.0, dy: -1.0 }))
        );
        assert_eq!(mouse.released(MouseButton::Left), None);
        assert_eq!(
            mouse.moved([200.0, 200.0]),
            None,
            "moving with no button held"
        );
    }

    #[test]
    fn a_right_drag_up_zooms_in_about_where_it_started() {
        let mut mouse = Mouse::default();
        mouse.pressed(MouseButton::Right, [300.0, 400.0]);
        let Some(Command::Zoom { factor, anchor }) = mouse.moved([300.0, 200.0]) else {
            panic!("right drag did not zoom");
        };
        assert!((factor - 2.0).abs() < 1e-5);
        assert_eq!(anchor, Anchor::Cursor { x: 300.0, y: 400.0 });
        assert_eq!(mouse.released(MouseButton::Right), None);
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
