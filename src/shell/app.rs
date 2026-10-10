//! The winit `ApplicationHandler`: turns input into [`Command`]s, owns all state and runs commands.

#![expect(dead_code, reason = "scaffold: most commands arrive with later issues")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowId};

use crate::browse::domain::ImageList;
use crate::browse::infrastructure::{Found, Open, Scan, ScanEvent};
use crate::image_file::domain::Orientation;
use crate::notices::domain::Notices;
use crate::notices::presentation::{self as notices, thousands};
use crate::prefs::domain::Preferences;
use crate::shell::command::Command;
use crate::shell::keymap;
use crate::shell::render::{self, ImageTexture, Renderer, UiFrame};
use crate::view::domain::View;

/// Sent to the event loop from other threads through its proxy.
pub enum AppEvent {
    /// `generation` tells a superseded scan's late events from the current scan's.
    Scan { generation: u64, event: ScanEvent },
}

/// Key of the notice that shows scan progress, then its result.
const SCAN_NOTICE: &str = "scan";

/// Window and GPU state exist from the first `resumed` on; Windows sends it once, at startup.
pub struct App {
    shown: Option<Shown>,
    proxy: EventLoopProxy<AppEvent>,
    /// Command-line paths, opened once the window exists to show progress.
    open_at_start: Option<Open>,
    /// Dropping it cancels the scan.
    scan: Option<Scan>,
    scan_generation: u64,
    images: Option<ImageList>,
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

impl App {
    pub fn new(proxy: EventLoopProxy<AppEvent>, paths: Vec<PathBuf>) -> Self {
        Self {
            shown: None,
            proxy,
            open_at_start: (!paths.is_empty()).then_some(Open::Paths(paths)),
            scan: None,
            scan_generation: 0,
            images: None,
        }
    }

    /// Starts a scan on its own thread, replacing (and so cancelling) any scan still running.
    fn open(&mut self, open: Open) {
        self.scan_generation += 1;
        let generation = self.scan_generation;
        let proxy = self.proxy.clone();
        // Preferences are not loaded yet; the default folder filter stands in.
        let folder_filter = Preferences::default().folder_filter;
        self.scan = Some(Scan::spawn(open, &folder_filter, move |event| {
            // Fails only once the event loop has exited, when nobody wants the result.
            let _ = proxy.send_event(AppEvent::Scan { generation, event });
        }));
    }

    fn scanned(&mut self, event: ScanEvent) {
        // Scans start only once the window exists.
        let Some(shown) = &mut self.shown else { return };
        let now = Instant::now();
        match event {
            ScanEvent::Progress(count) => {
                let text = format!("Scanning… {}", thousands(count));
                shown.notices.notify_as(SCAN_NOTICE, text, now);
            }
            ScanEvent::Done(found) => {
                self.scan = None;
                let Found {
                    list,
                    unreadable_folders,
                    missing,
                } = found;
                let text = match list.len() {
                    0 => "No images found".to_owned(),
                    1 => "1 image".to_owned(),
                    count => format!("{} images", thousands(count)),
                };
                shown.notices.notify_as(SCAN_NOTICE, text, now);
                if unreadable_folders > 0 {
                    let plural = if unreadable_folders == 1 { "" } else { "s" };
                    let text = format!(
                        "Skipped {} unreadable folder{plural}",
                        thousands(unreadable_folders)
                    );
                    shown.notices.notify(text, now);
                }
                if let Some(first) = missing.first() {
                    let mut text = format!("Not found: {}", first.display());
                    if missing.len() > 1 {
                        text += &format!(" and {} more", thousands(missing.len() - 1));
                    }
                    shown.notices.notify(text, now);
                }
                self.images = Some(list);
            }
        }
        shown.window.request_redraw();
    }
}

impl ApplicationHandler<AppEvent> for App {
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
        let (image_size, pixels) = render::stand_in();
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
        if let Some(open) = self.open_at_start.take() {
            self.open(open);
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Scan { generation, event } if generation == self.scan_generation => {
                self.scanned(event)
            }
            AppEvent::Scan { .. } => {}
        }
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
        let shown_notices = &mut self.notices;
        let output = self
            .egui
            .run_ui(input, |ui| notices::show(shown_notices, ui.ctx(), now));
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
