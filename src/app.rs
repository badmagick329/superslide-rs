//! The winit `ApplicationHandler`: turns input into [`Command`]s, owns all state and runs commands.

#![expect(dead_code, reason = "scaffold: most commands arrive with later issues")]

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowId};

use crate::decode::Orientation;
use crate::files::Transfer;
use crate::keymap;
use crate::library::{Direction, SortKey};
use crate::render::Renderer;
use crate::view::{Anchor, PanBy, Turn, View};

/// Everything the owner can ask for. Every row of `docs/context/keymap.md` maps to a variant;
/// rows that differ only by a parameter (R / L, M / C) share one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Next,
    Previous,
    /// First image of the adjacent group; does nothing unless grouped by subfolder.
    JumpGroup(Direction),
    ToggleSlideshow,
    Pan(PanBy),
    /// Multiplies the zoom by `factor`. Wheel, right drag, + / - and Ctrl+ / Ctrl- differ only in
    /// factor and anchor.
    Zoom {
        factor: f32,
        anchor: Anchor,
    },
    Fit,
    Rotate(Turn),
    ToggleMagnificationFilter,
    CycleInfoOverlay,
    ToggleFullscreen,
    /// Esc: leaves fullscreen, or quits when windowed.
    LeaveFullscreenOrQuit,
    Sort(SortKey),
    /// Gives a new random order even when already shuffled.
    Shuffle,
    ReverseSort,
    ToggleGrouping,
    Open {
        recursive: bool,
    },
    DeleteToRecycleBin,
    Transfer(Transfer),
    CopyToClipboard,
    PasteFromClipboard,
    TogglePreferences,
}

/// Window and GPU state exist from the first `resumed` on; Windows sends it once, at startup.
#[derive(Default)]
pub struct App {
    shown: Option<Shown>,
}

struct Shown {
    window: Arc<Window>,
    renderer: Renderer,
    view: View,
    mouse: keymap::Mouse,
    modifiers: ModifiersState,
    /// Last known cursor position in window pixels; wheel zoom anchors here.
    cursor: [f32; 2],
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.shown.is_some() {
            return;
        }
        let attributes = Window::default_attributes().with_title("SuperSlide-RS");
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => return fail(event_loop, &format!("Cannot open a window: {e}")),
        };
        let renderer = match Renderer::new(window.clone()) {
            Ok(renderer) => renderer,
            Err(e) => return fail(event_loop, &format!("Cannot start drawing: {e}")),
        };
        let size = window.inner_size();
        let view = View::new(
            renderer.image_size(),
            Orientation::Normal,
            0,
            [size.width, size.height],
        );
        self.shown = Some(Shown {
            window,
            renderer,
            view,
            mouse: keymap::Mouse::default(),
            modifiers: ModifiersState::empty(),
            cursor: [0.0, 0.0],
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(shown) = &mut self.shown else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                shown.renderer.resize(size.width, size.height);
                // A minimised window is 0 × 0; the view keeps its last real size.
                if size.width > 0 && size.height > 0 {
                    shown.view.set_window([size.width, size.height]);
                }
                shown.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let size = shown.window.inner_size();
                if size.width > 0 && size.height > 0 {
                    shown.renderer.draw(shown.view.transform());
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => shown.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let command = keymap::for_key(&event.logical_key, shown.modifiers, event.repeat);
                shown.run(command, event_loop);
            }
            WindowEvent::CursorMoved { position, .. } => {
                shown.cursor = [position.x as f32, position.y as f32];
                let command = shown.mouse.moved(shown.cursor);
                shown.run(command, event_loop);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let command = match state {
                    ElementState::Pressed => {
                        shown.mouse.pressed(button, shown.cursor);
                        None
                    }
                    ElementState::Released => shown.mouse.released(button),
                };
                shown.run(command, event_loop);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let command = keymap::for_wheel(delta, shown.cursor);
                shown.run(command, event_loop);
            }
            _ => {}
        }
    }
}

impl Shown {
    fn run(&mut self, command: Option<Command>, event_loop: &ActiveEventLoop) {
        let Some(command) = command else { return };
        let fullscreen = self.window.fullscreen().is_some();
        match command {
            // Borderless on whichever monitor holds the window: no mode switch, so no flicker.
            Command::ToggleFullscreen if !fullscreen => self
                .window
                .set_fullscreen(Some(Fullscreen::Borderless(self.window.current_monitor()))),
            Command::ToggleFullscreen | Command::LeaveFullscreenOrQuit if fullscreen => {
                self.window.set_fullscreen(None)
            }
            Command::LeaveFullscreenOrQuit => event_loop.exit(),
            Command::Pan(by) => self.view.pan(by),
            Command::Zoom { factor, anchor } => self.view.zoom(factor, anchor),
            Command::Fit => self.view.fit(),
            Command::Rotate(turn) => self.view.rotate(turn),
            _ => return,
        }
        self.window.request_redraw();
    }
}

/// Startup failures come before any overlay exists, and release builds have no console, so a
/// message box is the only way the owner hears about them.
fn fail(event_loop: &ActiveEventLoop, message: &str) {
    eprintln!("superslide: {message}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("SuperSlide-RS")
        .set_description(message)
        .show();
    event_loop.exit();
}
