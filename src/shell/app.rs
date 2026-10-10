//! The winit `ApplicationHandler`: turns input into [`Command`]s, owns all state and runs commands.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowId};

use crate::browse::domain::advance::AdvanceLimit;
use crate::browse::domain::{Direction, ImageList, Order, SortMode, folder};
use crate::browse::infrastructure::{Found, Open, Scan, ScanEvent};
use crate::browse::presentation::describe;
use crate::image_file::domain::{Format, Orientation};
use crate::load::domain::preload::{PreloadCache, preload_window};
use crate::load::domain::{DecodeError, Decoded};
use crate::load::infrastructure::workers::Workers;
use crate::load::presentation::error_card;
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
    /// A decode worker finished `path`.
    Decoded {
        path: PathBuf,
        result: Result<Decoded, DecodeError>,
    },
}

/// Key of the notice that shows scan progress, then its result.
const SCAN_NOTICE: &str = "scan";
const LOADING_NOTICE: &str = "loading";
/// Key of the notice naming the image list's order, so pressing sort keys in a row replaces it.
const ORDER_NOTICE: &str = "order";
const TITLE: &str = "SuperSlide-RS";
/// Waits shorter than this pass without a notice: a flash of `Loading…` on every image change
/// would be noise.
const LOADING_DELAY: Duration = Duration::from_millis(150);

/// One image in the preload cache. The texture is shared with [`Showing`], so evicting the image
/// on screen does not free it mid-display.
struct Cached {
    texture: Rc<ImageTexture>,
    orientation: Orientation,
}

/// What is on screen. It changes only once the current image is ready, so an image change never
/// shows a blank frame.
enum Showing {
    Image {
        /// `None` for the stand-in shown before the first image opens.
        path: Option<PathBuf>,
        texture: Rc<ImageTexture>,
        view: View,
    },
    Error {
        path: PathBuf,
        file_name: String,
        reason: String,
    },
}

impl Showing {
    fn path(&self) -> Option<&Path> {
        match self {
            Showing::Image { path, .. } => path.as_deref(),
            Showing::Error { path, .. } => Some(path),
        }
    }
}

/// The current image has been wanted but not shown since `since`.
struct Waiting {
    since: Instant,
    /// `Loading…` is up.
    noticed: bool,
}

/// Window and GPU state exist from the first `resumed` on; Windows sends it once, at startup.
pub struct App {
    shown: Option<Shown>,
    proxy: EventLoopProxy<AppEvent>,
    /// Command-line paths, opened once the window exists to show progress.
    open_at_start: Option<Open>,
    /// Dropping it cancels the scan.
    scan: Option<Scan>,
    scan_generation: u64,
    /// Files dropped since the loop last went idle. One drag delivers all its files before then,
    /// so they open together as one burst.
    dropped: Vec<PathBuf>,
}

struct Shown {
    window: Arc<Window>,
    /// The last non-zero inner size: a minimised window reports 0 × 0, which no view can fit.
    window_size: [u32; 2],
    renderer: Renderer,
    showing: Showing,
    images: Option<ImageList>,
    /// Which way the owner last moved; the preload window leans this way.
    direction: Direction,
    advance: AdvanceLimit,
    /// Screen rotation per path, until the app closes; across opens too.
    turns: HashMap<PathBuf, u8>,
    cache: PreloadCache<Cached>,
    workers: Workers,
    waiting: Option<Waiting>,
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
            dropped: Vec::new(),
        }
    }

    /// Opening routes need `App`, which owns the scan; everything else goes to `Shown::run`.
    fn run(&mut self, command: Option<Command>, event_loop: &ActiveEventLoop) {
        let Some(shown) = &mut self.shown else { return };
        match command {
            Some(Command::Open { recursive }) => {
                if let Some(open) = shown.pick(recursive) {
                    self.open(open);
                }
            }
            command => shown.run(command, event_loop),
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
                shown.opened(list, now);
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
        let attributes = Window::default_attributes().with_title(TITLE);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => return fail(event_loop, &format!("Cannot open a window: {e}")),
        };
        let renderer = match Renderer::new(window.clone()) {
            Ok(renderer) => renderer,
            Err(e) => return fail(event_loop, &format!("Cannot start drawing: {e}")),
        };
        let (image_size, pixels) = render::stand_in();
        let texture = Rc::new(renderer.upload(image_size, &pixels));
        let size = window.inner_size();
        let window_size = [size.width.max(1), size.height.max(1)];
        let view = View::new(texture.size(), Orientation::Normal, 0, window_size);
        let proxy = self.proxy.clone();
        let workers = Workers::spawn(renderer.max_texture_dimension, move |path, result| {
            // Fails only once the event loop has exited, when nobody wants the result.
            let _ = proxy.send_event(AppEvent::Decoded { path, result });
        });
        // Preferences are not loaded yet; the defaults stand in.
        let preferences = Preferences::default();
        let budget = u64::from(preferences.preload_budget_mb) << 20;
        let advance_limit = Duration::from_millis(preferences.advance_limit_ms.into());
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
            window_size,
            renderer,
            showing: Showing::Image {
                path: None,
                texture,
                view,
            },
            images: None,
            direction: Direction::Forward,
            advance: AdvanceLimit::new(advance_limit),
            turns: HashMap::new(),
            cache: PreloadCache::new(budget),
            workers,
            waiting: None,
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
            AppEvent::Decoded { path, result } => {
                if let Some(shown) = &mut self.shown {
                    shown.decoded(path, result);
                }
            }
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
                // A minimised window is 0 × 0; views keep the last real size.
                if size.width > 0 && size.height > 0 {
                    shown.window_size = [size.width, size.height];
                    if let Some(view) = shown.view() {
                        view.set_window([size.width, size.height]);
                    }
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
            WindowEvent::KeyboardInput { event, .. } => match event.state {
                ElementState::Pressed => {
                    let command =
                        keymap::for_key(&event.logical_key, shown.modifiers, event.repeat);
                    self.run(command, event_loop);
                }
                ElementState::Released => {
                    if let Some(direction) = keymap::released(&event.logical_key) {
                        shown.advance.released(direction);
                    }
                }
            },
            WindowEvent::DroppedFile(path) => self.dropped.push(path),
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
                self.run(command, event_loop);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let command = keymap::for_wheel(delta, shown.cursor);
                shown.run(command, event_loop);
            }
            _ => {}
        }
    }

    /// Opens a burst of drops. Sleeps until a notice starts fading, `Loading…` is due, a held key
    /// may advance or egui asks for a frame; redraws every frame while a notice fades.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.dropped.is_empty() {
            let paths = std::mem::take(&mut self.dropped);
            self.open(Open::Paths(paths));
        }
        let Some(shown) = &mut self.shown else { return };
        let now = Instant::now();
        if let Some(direction) = shown.advance.tick(now) {
            shown.navigate(direction);
        }
        if let Some(waiting) = &mut shown.waiting
            && !waiting.noticed
            && now >= waiting.since + LOADING_DELAY
        {
            waiting.noticed = true;
            shown.notices.notify_as(LOADING_NOTICE, "Loading…", now);
            shown.window.request_redraw();
        }
        let loading_at = shown
            .waiting
            .as_ref()
            .filter(|waiting| !waiting.noticed)
            .map(|waiting| waiting.since + LOADING_DELAY);
        let next = [
            shown.notices.next_change(now),
            shown.egui_repaint_at,
            loading_at,
            shown.advance.due(),
        ]
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
    fn view(&mut self) -> Option<&mut View> {
        match &mut self.showing {
            Showing::Image { view, .. } => Some(view),
            Showing::Error { .. } => None,
        }
    }

    /// A finished scan replaces the image list. Cached images it still holds are kept, at their
    /// new positions. An open that found nothing keeps the old list: its notice says so, and the
    /// owner carries on where they were.
    fn opened(&mut self, list: ImageList, now: Instant) {
        if list.len() == 0 {
            return;
        }
        self.cache
            .reindex(list.entries().iter().map(|entry| entry.path.as_path()));
        self.images = Some(list);
        self.direction = Direction::Forward;
        self.retarget();
        self.show_current(now);
    }

    fn navigate(&mut self, direction: Direction) {
        let Some(images) = &mut self.images else {
            return;
        };
        let wrapped = images.step(direction);
        self.moved(direction, wrapped);
    }

    /// Does nothing unless grouped by subfolder.
    fn jump_group(&mut self, direction: Direction) {
        let Some(wrapped) = self
            .images
            .as_mut()
            .and_then(|images| images.jump_group(direction))
        else {
            return;
        };
        self.moved(direction, wrapped);
    }

    /// The current image changed; wrapping round an end is easy to miss, so it gets a notice.
    fn moved(&mut self, direction: Direction, wrapped: bool) {
        let now = Instant::now();
        if wrapped {
            let text = match direction {
                Direction::Forward => "Wrapped to the first image",
                Direction::Backward => "Wrapped to the last image",
            };
            self.notices.notify(text, now);
            self.window.request_redraw();
        }
        self.direction = direction;
        self.retarget();
        self.show_current(now);
    }

    /// Re-sorts in memory; the current image stays current, so nothing on screen changes but the
    /// notice. The preload window moves to the current image's new neighbours.
    fn reorder(&mut self, change: impl FnOnce(Order) -> Order) {
        let Some(images) = &mut self.images else {
            return;
        };
        let order = change(images.order());
        images.set_order(order);
        self.cache
            .reindex(images.entries().iter().map(|entry| entry.path.as_path()));
        self.retarget();
        self.notices
            .notify_as(ORDER_NOTICE, describe(order), Instant::now());
        self.window.request_redraw();
    }

    /// The native dialog for O or Shift+O, opening at the current image's folder. It is modal,
    /// so the event loop waits for it. `None` when cancelled.
    fn pick(&self, recursive: bool) -> Option<Open> {
        let mut dialog = rfd::FileDialog::new().set_parent(&*self.window);
        if let Some(path) = self.showing.path() {
            dialog = dialog.set_directory(folder(path));
        }
        if recursive {
            dialog
                .set_title("Open a folder and its subfolders")
                .pick_folder()
                .map(Open::Recursive)
        } else {
            dialog
                .add_filter("Images", &Format::EXTENSIONS)
                .pick_file()
                .map(|file| Open::Paths(vec![file]))
        }
    }

    /// Points the preload cache and the workers at the window around the current image.
    fn retarget(&mut self) {
        let Some(images) = &self.images else { return };
        let entries = images.entries();
        let window = preload_window(entries.len(), images.current_position(), self.direction)
            .into_iter()
            .map(|position| (position, entries[position].path.clone()))
            .collect();
        self.cache.retarget(entries.len(), window);
        self.workers.request(self.cache.missing());
    }

    /// Swaps in the current image if it is ready; otherwise the previous one stays on screen and
    /// the wait starts.
    fn show_current(&mut self, now: Instant) {
        let Some(path) = self
            .images
            .as_ref()
            .and_then(ImageList::current)
            .map(|entry| &entry.path)
        else {
            return;
        };
        if self.showing.path() == Some(path) {
            return;
        }
        self.showing = match self.cache.get(path) {
            None => {
                self.waiting.get_or_insert(Waiting {
                    since: now,
                    noticed: false,
                });
                return;
            }
            Some(Ok(cached)) => Showing::Image {
                path: Some(path.clone()),
                texture: cached.texture.clone(),
                view: View::new(
                    cached.texture.size(),
                    cached.orientation,
                    self.turns.get(path).copied().unwrap_or(0),
                    self.window_size,
                ),
            },
            Some(Err(error)) => Showing::Error {
                path: path.clone(),
                file_name: file_name(path),
                reason: error.to_string(),
            },
        };
        // The title follows the screen, not the image list, so it never names an image still
        // loading.
        self.window
            .set_title(&format!("{} — {TITLE}", file_name(path)));
        self.waiting = None;
        self.notices.dismiss(LOADING_NOTICE);
        self.window.request_redraw();
    }

    /// Uploads a decoded image if the cache will keep it; its CPU pixels are freed either way.
    fn decoded(&mut self, path: PathBuf, result: Result<Decoded, DecodeError>) {
        self.workers.finished(&path);
        // Gone from the image list since it was requested: a newer open replaced it.
        let Some(position) = self
            .images
            .as_ref()
            .and_then(|images| images.position(&path))
        else {
            return;
        };
        let (bytes, loaded) = match result {
            Ok(decoded) => {
                let bytes = render::texture_bytes(decoded.size);
                if !self.cache.keeps(&path, bytes) {
                    return;
                }
                let texture = self.renderer.upload(decoded.size, &decoded.pixels);
                let cached = Cached {
                    texture: Rc::new(texture),
                    orientation: decoded.orientation,
                };
                (bytes, Ok(cached))
            }
            Err(error) => (0, Err(error)),
        };
        self.cache.insert(path, position, bytes, loaded);
        // Debug builds show stderr: this is how the budget is checked against Task Manager.
        eprintln!(
            "superslide: preload cache holds {} MB",
            self.cache.used() >> 20
        );
        self.show_current(Instant::now());
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let input = self.egui_input.take_egui_input(&self.window);
        let showing = &self.showing;
        let shown_notices = &mut self.notices;
        let output = self.egui.run_ui(input, |ui| {
            if let Showing::Error {
                file_name, reason, ..
            } = showing
            {
                error_card(ui.ctx(), file_name, reason);
            }
            notices::show(shown_notices, ui.ctx(), now);
        });
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
        let image = match &self.showing {
            Showing::Image { texture, view, .. } => Some((&**texture, view.transform())),
            Showing::Error { .. } => None,
        };
        self.renderer.draw(image, ui);
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
            // Each shows the image itself once it is ready; nothing to redraw until then.
            Command::Advance { direction, repeat } => {
                let now = Instant::now();
                if !repeat {
                    self.advance.pressed(now);
                } else if !self.advance.repeated(direction, now) {
                    return;
                }
                return self.navigate(direction);
            }
            Command::JumpGroup(direction) => return self.jump_group(direction),
            Command::Sort(key) => {
                return self.reorder(|order| Order {
                    mode: SortMode::By(key),
                    ..order
                });
            }
            Command::Shuffle => {
                return self.reorder(|order| Order {
                    mode: SortMode::new_shuffle(),
                    ..order
                });
            }
            Command::ReverseSort => {
                return self.reorder(|order| Order {
                    reversed: !order.reversed,
                    ..order
                });
            }
            Command::ToggleGrouping => {
                return self.reorder(|order| Order {
                    grouped: !order.grouped,
                    ..order
                });
            }
            // View commands do nothing on an error card.
            Command::Pan(by) => self.view().into_iter().for_each(|view| view.pan(by)),
            Command::Zoom { factor, anchor } => self
                .view()
                .into_iter()
                .for_each(|view| view.zoom(factor, anchor)),
            Command::Fit => self.view().into_iter().for_each(View::fit),
            Command::Rotate(turn) => {
                if let Showing::Image { path, view, .. } = &mut self.showing {
                    view.rotate(turn);
                    // The stand-in has no path, so its rotation is not kept.
                    if let Some(path) = path {
                        self.turns.insert(path.clone(), view.turns());
                    }
                }
            }
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

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
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
