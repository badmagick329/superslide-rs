//! Maps keys and mouse gestures to [`Command`], in the keymap the preferences choose. The only
//! place bindings are decided: [`classic`] and [`vim`] decide keys, `for_wheel` and `Mouse` the
//! mouse, which both keymaps share, and each keymap's `BINDINGS` describes them for the help
//! overlay. Tests hold those and `docs/context/keymap.md` together, so none can change alone.

mod classic;
mod vim;

use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::browse::domain::{Direction, Travel};
use crate::help::domain::{Binding, Scroll, Section};
use crate::prefs::domain::Keymap;
use crate::prefs::presentation::PreferenceKeys;
use crate::shell::command::Command;
use crate::view::domain::{Anchor, PanBy};
use crate::welcome::presentation::OpeningKeys;

/// Zoom per wheel notch and per + / - press.
const ZOOM_STEP: f32 = 1.25;
/// Share of the window one pan key press moves.
const KEY_PAN: f32 = 0.1;
/// Movement while a button is down, in window pixels, that still counts as a click. Hands
/// jitter; a click must not nudge the image.
const CLICK_SLOP: f32 = 4.0;
/// Upward right-drag distance, in window pixels, that doubles the zoom.
const DRAG_PX_PER_DOUBLING: f32 = 200.0;
/// Touchpads report pixels, not notches; this many count as one notch.
const PIXELS_PER_NOTCH: f32 = 50.0;
/// Lines the help overlay scrolls per wheel notch.
const HELP_LINES_PER_NOTCH: f32 = 3.0;

/// The keyboard in the keymap in use, with what Vim keys have typed so far.
#[derive(Debug)]
pub struct Keys {
    keymap: Keymap,
    vim: vim::Vim,
}

impl Keys {
    pub fn new(keymap: Keymap) -> Self {
        Self {
            keymap,
            vim: vim::Vim::default(),
        }
    }

    /// From the preferences screen. A half-typed Vim key is dropped, so it cannot complete in
    /// the other keymap.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        if keymap != self.keymap {
            *self = Self::new(keymap);
        }
    }

    /// The command a key press runs, if any. `repeat` is true for auto-repeat while the key is
    /// held.
    pub fn pressed(
        &mut self,
        key: &Key,
        modifiers: ModifiersState,
        repeat: bool,
    ) -> Option<Command> {
        match self.keymap {
            Keymap::Classic => classic::for_key(key, modifiers, repeat),
            Keymap::Vim => self.vim.pressed(key, modifiers, repeat),
        }
    }

    /// The direction a key let go of was moving, whatever the modifiers: a repeat waiting under
    /// the advance limit must not move after it.
    pub fn released(&self, key: &Key) -> Option<Direction> {
        match self.keymap {
            Keymap::Classic => classic::released(key),
            Keymap::Vim => vim::released(key),
        }
    }

    /// While the help overlay is open it takes every key: the ones that act on it give a
    /// [`HelpKey`], and every other key does nothing, so a key pressed at the list never runs its
    /// binding underneath.
    pub fn help_key(
        &mut self,
        key: &Key,
        modifiers: ModifiersState,
        repeat: bool,
    ) -> Option<HelpKey> {
        match self.keymap {
            Keymap::Classic => classic::for_help_key(key, modifiers, repeat),
            Keymap::Vim => self.vim.help_key(key, modifiers, repeat),
        }
    }

    /// While the preferences screen is open it takes every key; only the ones that close it act.
    pub fn closes_preferences(&self, key: &Key, modifiers: ModifiersState, repeat: bool) -> bool {
        match self.keymap {
            Keymap::Classic => classic::closes_preferences(key, modifiers, repeat),
            Keymap::Vim => vim::closes_preferences(key, modifiers, repeat),
        }
    }

    pub fn bindings(&self) -> &'static [Row] {
        match self.keymap {
            Keymap::Classic => classic::BINDINGS,
            Keymap::Vim => vim::BINDINGS,
        }
    }

    pub fn hints(&self) -> &'static Hints {
        match self.keymap {
            Keymap::Classic => &classic::HINTS,
            Keymap::Vim => &vim::HINTS,
        }
    }

    /// A count or the first key of a two-key binding, as typed; Vim shows it in the same corner.
    pub fn pending(&self) -> Option<String> {
        self.vim.pending()
    }
}

/// The keys other screens name, in the keymap in use.
pub struct Hints {
    pub openings: OpeningKeys,
    /// The help overlay's line under its filter box.
    pub help: &'static str,
    pub preferences: PreferenceKeys,
}

/// What a key press does while the help overlay is open and its filter box is not focused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HelpKey {
    Close,
    FocusFilter,
    Scroll(Scroll),
}

/// The help overlay keys both keymaps share. They repeat while held.
fn help_scroll(key: &Key) -> Option<Scroll> {
    Some(match key {
        Key::Named(NamedKey::ArrowUp) => Scroll::Lines(-1.0),
        Key::Named(NamedKey::ArrowDown) => Scroll::Lines(1.0),
        Key::Named(NamedKey::PageUp) => Scroll::Pages(-1.0),
        Key::Named(NamedKey::PageDown) => Scroll::Pages(1.0),
        Key::Named(NamedKey::Home) => Scroll::Top,
        Key::Named(NamedKey::End) => Scroll::Bottom,
        _ => return None,
    })
}

/// The wheel scrolls the help overlay, wherever the cursor is, instead of zooming.
pub fn for_help_wheel(delta: MouseScrollDelta) -> Option<Scroll> {
    let notches = notches(delta);
    // Wheel up gives positive notches and scrolls towards the top.
    (notches != 0.0).then(|| Scroll::Lines(-notches * HELP_LINES_PER_NOTCH))
}

const fn advance(direction: Direction, repeat: bool) -> Command {
    Command::Advance {
        travel: Travel::Step(direction),
        repeat,
    }
}

const fn zoom(factor: f32) -> Command {
    Command::Zoom {
        factor,
        anchor: Anchor::Centre,
    }
}

/// Pan keys scroll the view: Right shows more of the right side, so the image moves left.
const fn pan_key(direction: NamedKey) -> Command {
    let (dx, dy) = match direction {
        NamedKey::ArrowLeft => (KEY_PAN, 0.0),
        NamedKey::ArrowRight => (-KEY_PAN, 0.0),
        NamedKey::ArrowUp => (0.0, KEY_PAN),
        _ => (0.0, -KEY_PAN),
    };
    Command::Pan(PanBy::WindowFraction { dx, dy })
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

/// One row of a keymap: the binding as the help overlay shows it, and sample inputs with the
/// command each must give.
pub struct Row {
    pub binding: Binding,
    /// Replayed by the tests; every row has at least one.
    #[cfg_attr(not(test), expect(dead_code, reason = "only tests replay probes"))]
    probes: &'static [Probe],
}

/// An input a test can replay, with the command it must give.
#[cfg_attr(not(test), expect(dead_code, reason = "only tests replay probes"))]
struct Probe(Input, Command);

#[cfg_attr(not(test), expect(dead_code, reason = "only tests replay probes"))]
enum Input {
    Named(NamedKey, ModifiersState),
    Char(&'static str, ModifiersState),
    /// Vim keys in Vim's notation, pressed in turn: `5G`, `gg`, `<C-d>`, `<Space>`. Only the
    /// last press may give a command.
    Keys(&'static str),
    /// Press and release at `PROBE_AT` without moving.
    Click(MouseButton),
    /// Press at `PROBE_AT`, then move by the offset.
    Drag(MouseButton, [f32; 2]),
    /// Wheel notches with the cursor at `PROBE_AT`.
    Wheel(f32),
}

/// Where mouse probes happen, so the commands they expect can name their anchor.
const PROBE_AT: [f32; 2] = [100.0, 100.0];

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

/// The mouse probes both keymaps share.
const CLICK_NEXT: Probe = Probe(
    Input::Click(MouseButton::Right),
    advance(Direction::Forward, false),
);
const CLICK_PREVIOUS: Probe = Probe(
    Input::Click(MouseButton::Left),
    advance(Direction::Backward, false),
);
const DRAG_PAN: Probe = Probe(
    Input::Drag(MouseButton::Left, [10.0, 0.0]),
    Command::Pan(PanBy::Pixels { dx: 10.0, dy: 0.0 }),
);
const DRAG_ZOOM: Probe = Probe(
    Input::Drag(MouseButton::Right, [0.0, -DRAG_PX_PER_DOUBLING]),
    zoom_at_probe(2.0),
);
const WHEEL_ZOOM: Probe = Probe(Input::Wheel(1.0), zoom_at_probe(ZOOM_STEP));

const fn zoom_at_probe(factor: f32) -> Command {
    Command::Zoom {
        factor,
        anchor: Anchor::Cursor {
            x: PROBE_AT[0],
            y: PROBE_AT[1],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mouse probe's command; `None` for key probes, which each keymap replays itself.
    pub(super) fn replay_mouse(input: &Input) -> Option<Option<Command>> {
        let mut mouse = Mouse::default();
        Some(match *input {
            Input::Click(button) => {
                mouse.pressed(button, PROBE_AT);
                mouse.released(button)
            }
            Input::Drag(button, [dx, dy]) => {
                mouse.pressed(button, PROBE_AT);
                mouse.moved([PROBE_AT[0] + dx, PROBE_AT[1] + dy])
            }
            Input::Wheel(notches) => for_wheel(MouseScrollDelta::LineDelta(0.0, notches), PROBE_AT),
            _ => return None,
        })
    }

    /// Each keymap with how its tests replay a probe, the row each command belongs to, and the
    /// header of its table in `docs/context/keymap.md`.
    type Replay = fn(&Input) -> Option<Command>;
    type RowOf = fn(Command) -> &'static str;
    const KEYMAPS: [(Keymap, Replay, RowOf, &str); 2] = [
        (
            Keymap::Classic,
            classic::tests::replay,
            classic::tests::row_of,
            "| Input | Command |",
        ),
        (
            Keymap::Vim,
            vim::tests::replay,
            vim::tests::row_of,
            "| Vim input | Command |",
        ),
    ];

    #[test]
    fn every_probe_gives_its_command_on_its_row() {
        for (keymap, replay, row_of, _) in KEYMAPS {
            for row in Keys::new(keymap).bindings() {
                let keys = row.binding.keys;
                for Probe(input, command) in row.probes {
                    assert_eq!(
                        replay(input),
                        Some(*command),
                        "{keymap:?} probe on row {keys}"
                    );
                    assert_eq!(
                        row_of(*command),
                        keys,
                        "{keymap:?}: {command:?} probed on the wrong row"
                    );
                }
            }
        }
    }

    #[test]
    fn every_row_has_probes() {
        for (keymap, ..) in KEYMAPS {
            let unprobed: Vec<_> = Keys::new(keymap)
                .bindings()
                .iter()
                .filter(|row| row.probes.is_empty())
                .map(|row| row.binding.keys)
                .collect();
            assert!(unprobed.is_empty(), "{keymap:?}: {unprobed:?}");
        }
    }

    #[test]
    fn the_keymap_doc_lists_the_same_rows() {
        let doc = include_str!("../../../docs/context/keymap.md");
        for (keymap, _, _, header) in KEYMAPS {
            let table: Vec<(&str, &str)> = doc
                .lines()
                .skip_while(|line| *line != header)
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
            assert!(!table.is_empty(), "keymap.md has no {keymap:?} table");
            let rows: Vec<_> = Keys::new(keymap)
                .bindings()
                .iter()
                .map(|row| (row.binding.keys, row.binding.action))
                .collect();
            assert_eq!(table, rows[..table.len()], "{keymap:?}");
            // The doc mentions F9 in prose, as release builds lack it.
            let extra: Vec<_> = rows[table.len()..].iter().map(|row| row.0).collect();
            let expected: &[&str] = if cfg!(debug_assertions) { &["F9"] } else { &[] };
            assert_eq!(extra, expected, "{keymap:?}");
        }
    }

    #[test]
    fn each_section_is_one_run() {
        for (keymap, ..) in KEYMAPS {
            let mut seen = Vec::new();
            for row in Keys::new(keymap).bindings() {
                let section = row.binding.section;
                if seen.last() != Some(&section) {
                    assert!(!seen.contains(&section), "{keymap:?}: {section:?} is split");
                    seen.push(section);
                }
            }
        }
    }

    #[test]
    fn switching_keymaps_drops_a_half_typed_key() {
        let mut keys = Keys::new(Keymap::Vim);
        let g = Key::Character("g".into());
        assert_eq!(keys.pressed(&g, ModifiersState::empty(), false), None);
        assert_eq!(keys.pending().as_deref(), Some("g"));
        keys.set_keymap(Keymap::Classic);
        keys.set_keymap(Keymap::Vim);
        assert_eq!(keys.pending(), None);
        assert_eq!(
            keys.pressed(&g, ModifiersState::empty(), false),
            None,
            "a fresh g, not the second of gg"
        );
    }

    #[test]
    fn the_wheel_scrolls_help_down_for_a_notch_down() {
        assert_eq!(
            for_help_wheel(MouseScrollDelta::LineDelta(0.0, -1.0)),
            Some(Scroll::Lines(3.0))
        );
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
}
