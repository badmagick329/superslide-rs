//! The winit `ApplicationHandler`: turns input into [`Command`]s, owns all state and runs commands.

#![expect(dead_code, reason = "scaffold: most commands arrive with later issues")]

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowId};

use crate::decode::Orientation;
use crate::files::Transfer;
use crate::keymap;
use crate::library::{Direction, SortKey};
use crate::render::{ImageTexture, Renderer, UiFrame};
use crate::ui::Notices;
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
    /// F9 in debug builds: a fixed notice, for checking notice timing and stacking by hand.
    #[cfg(debug_assertions)]
    TestNotice,
}

/// Window and GPU state exist from the first `resumed` on; Windows sends it once, at startup.
#[derive(Default)]
pub struct App {
    shown: Option<Shown>,
}

struct Shown {
    window: Arc<Window>,
    renderer: Renderer,
    image: ImageTexture,
    view: View,
    egui: egui::Context,
    egui_input: egui_winit::State,
    notices: Notices,
    /// When egui asked to be run again. It does after a new area's first frame, which it spends
    /// measuring the area with nothing visible.
    egui_repaint_at: Option<Instant>,
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
        let (image_size, pixels) = crate::render::stand_in();
        let image = renderer.upload(image_size, &pixels);
        let size = window.inner_size();
        let view = View::new(
            image.size(),
            Orientation::Normal,
            0,
            [size.width, size.height],
        );
        let egui = egui::Context::default();
        let egui_input = egui_winit::State::new(
            egui.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(renderer.max_texture_dimension as usize),
        );
        self.shown = Some(Shown {
            window,
            renderer,
            image,
            view,
            egui,
            egui_input,
            notices: Notices::default(),
            egui_repaint_at: None,
            mouse: keymap::Mouse::default(),
            modifiers: ModifiersState::empty(),
            cursor: [0.0, 0.0],
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(shown) = &mut self.shown else { return };
        // egui sees every event, so its pointer and modifier state stay current, but takes one only
        // while a widget has focus; otherwise all input belongs to the keymap.
        let response = shown.egui_input.on_window_event(&shown.window, &event);
        if response.consumed && shown.egui.memory(|memory| memory.focused().is_some()) {
            if response.repaint {
                shown.window.request_redraw();
            }
            return;
        }
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
                    shown.redraw();
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

    /// Sleeps until a notice starts fading or egui asks for a frame; redraws every frame while a
    /// notice fades.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(shown) = &self.shown else { return };
        let now = Instant::now();
        let next = [shown.notices.next_change(now), shown.egui_repaint_at]
            .into_iter()
            .flatten()
            .min();
        let flow = match next {
            Some(at) if at > now => ControlFlow::WaitUntil(at),
            Some(_) => {
                shown.window.request_redraw();
                ControlFlow::Wait
            }
            None => ControlFlow::Wait,
        };
        event_loop.set_control_flow(flow);
    }
}

impl Shown {
    fn redraw(&mut self) {
        let now = Instant::now();
        let input = self.egui_input.take_egui_input(&self.window);
        let notices = &mut self.notices;
        let output = self.egui.run_ui(input, |ui| notices.show(ui.ctx(), now));
        self.egui_input
            .handle_platform_output(&self.window, output.platform_output);
        // `Duration::MAX` means "no repaint needed"; adding it overflows to `None`.
        self.egui_repaint_at = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .and_then(|viewport| now.checked_add(viewport.repaint_delay));
        let ui = UiFrame {
            primitives: self.egui.tessellate(output.shapes, output.pixels_per_point),
            textures: output.textures_delta,
            pixels_per_point: output.pixels_per_point,
        };
        self.renderer.draw(&self.image, self.view.transform(), ui);
    }

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
            #[cfg(debug_assertions)]
            Command::TestNotice => self.notices.notify("Test notice", Instant::now()),
            Command::ToggleMagnificationFilter => {
                let filter = &mut self.renderer.magnification_filter;
                *filter = filter.toggled();
            }
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
