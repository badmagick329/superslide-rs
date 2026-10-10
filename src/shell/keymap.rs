//! Maps keys and mouse gestures to [`Command`]. The only place bindings are decided; the table is
//! `docs/context/keymap.md`.

use winit::event::{MouseButton, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};

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
        // Held, these will repeat under the advance limit (SSR-12); until then, once per press.
        Key::Named(NamedKey::Space) if plain => (Command::Next, false),
        Key::Named(NamedKey::Backspace) if plain => (Command::Previous, false),
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
                _ if !plain => return None,
                "0" => (Command::Fit, false),
                "r" | "R" => (Command::Rotate(Turn::Right), false),
                "l" | "L" => (Command::Rotate(Turn::Left), false),
                _ => return None,
            }
        }
        _ => return None,
    };
    // Held toggles would flicker between states, and a held Esc would leave fullscreen then quit.
    (repeats || !repeat).then_some(command)
}

fn zoom(factor: f32) -> Command {
    Command::Zoom {
        factor,
        anchor: Anchor::Centre,
    }
}

/// Wheel up zooms in, about the cursor.
pub fn for_wheel(delta: MouseScrollDelta, cursor: [f32; 2]) -> Option<Command> {
    let notches = match delta {
        MouseScrollDelta::LineDelta(_, y) => y,
        MouseScrollDelta::PixelDelta(p) => p.y as f32 / PIXELS_PER_NOTCH,
    };
    (notches != 0.0).then(|| Command::Zoom {
        factor: ZOOM_STEP.powf(notches),
        anchor: Anchor::Cursor {
            x: cursor[0],
            y: cursor[1],
        },
    })
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
            MouseButton::Left => Command::Previous,
            _ => Command::Next,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_with_jitter_does_not_pan() {
        let mut mouse = Mouse::default();
        mouse.pressed(MouseButton::Left, [100.0, 100.0]);
        for at in [[102.0, 101.0], [97.0, 98.0], [104.0, 100.0], [100.0, 96.0]] {
            assert_eq!(mouse.moved(at), None, "jitter to {at:?} panned");
        }
        assert_eq!(mouse.released(MouseButton::Left), Some(Command::Previous));

        mouse.pressed(MouseButton::Right, [100.0, 100.0]);
        assert_eq!(mouse.moved([103.0, 102.0]), None);
        assert_eq!(mouse.released(MouseButton::Right), Some(Command::Next));
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
        // Ctrl+R and Ctrl+arrows belong to other bindings, not rotate and pan.
        assert_eq!(for_key(&Key::Character("r".into()), ctrl, false), None);
        assert_eq!(for_key(&Key::Named(NamedKey::ArrowLeft), ctrl, false), None);
    }
}
