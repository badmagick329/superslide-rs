//! The winit `ApplicationHandler`: turns input into [`Command`]s, owns all state and runs commands.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::monitor::MonitorHandle;
use winit::platform::windows::{IconExtWindows, WindowExtWindows};
use winit::window::{Fullscreen, Icon, Window, WindowId};

use crate::browse::domain::advance::AdvanceLimit;
use crate::browse::domain::slideshow::Slideshow;
use crate::browse::domain::{Direction, ImageList, Order, Resume, SortMode, folder};
use crate::browse::infrastructure::{Found, Open, Scan, ScanEvent};
use crate::browse::presentation::describe;
use crate::clipboard::infrastructure::{ClipboardThread, Done, Loading, Request};
use crate::clipboard::presentation as clipboard;
use crate::file_actions::domain::{Action, Failure, Transfer};
use crate::file_actions::infrastructure::FileActions;
use crate::file_actions::presentation as file_actions;
use crate::help::domain::{Binding, Help};
use crate::help::presentation as help;
use crate::image_file::domain::{Format, Orientation};
use crate::info_overlay::domain::{Details, FileFacts, InfoOverlay, Listed, Pixels, Subject};
use crate::info_overlay::infrastructure::local_time;
use crate::info_overlay::presentation as info_overlay;
use crate::load::domain::preload::{PreloadCache, preload_window};
use crate::load::domain::{DecodeError, Decoded};
use crate::load::infrastructure::workers::Workers;
use crate::load::infrastructure::{decode, shrink_to_fit};
use crate::load::presentation::error_card;
use crate::notices::domain::Notices;
use crate::notices::presentation::{self as notices, thousands};
use crate::prefs::domain::session::{self, Monitor, Placement, Route, Session};
use crate::prefs::domain::{Preferences, Screen};
use crate::prefs::infrastructure::TomlFile;
use crate::prefs::presentation as preferences;
use crate::shell::command::Command;
use crate::shell::keymap::{self, HelpKey};
use crate::shell::render::{self, ImageTexture, Renderer, UiFrame};
use crate::view::domain::{MagnificationFilter, View};
use crate::welcome::presentation as welcome;

/// Sent to the event loop from other threads through its proxy.
pub enum AppEvent {
    /// `generation` tells a superseded scan's late events from the current scan's.
    Scan { generation: u64, event: ScanEvent },
    /// A decode worker finished `path`.
    Decoded {
        path: PathBuf,
        result: Result<Decoded, DecodeError>,
    },
    /// The file action thread finished `action` on `path`.
    Acted {
        path: PathBuf,
        action: Action,
        result: Result<(), Failure>,
    },
    /// The clipboard thread finished a copy or a paste.
    Clipboard(Done),
}

/// Key of the notice that shows scan progress, then its result.
const SCAN_NOTICE: &str = "scan";
const LOADING_NOTICE: &str = "loading";
/// Key of the notice naming the image list's order, so pressing sort keys in a row replaces it.
const ORDER_NOTICE: &str = "order";
const SLIDESHOW_NOTICE: &str = "slideshow";
/// Key of the notice for Ctrl+C and Ctrl+V, so `Copying…` is replaced by its result.
const CLIPBOARD_NOTICE: &str = "clipboard";
const NOT_A_FILE: &str = "This image is not a file";
const TITLE: &str = "SuperSlide-RS";
/// The app icon's resource ordinal, from `assets/superslide.rc`.
const ICON_RESOURCE: u16 = 1;
/// Waits shorter than this pass without a notice: a flash of `Loading…` on every image change
/// would be noise.
const LOADING_DELAY: Duration = Duration::from_millis(150);
/// A changed session is written at most this often: a held Space changes it many times a second.
const SESSION_INTERVAL: Duration = Duration::from_secs(10);

/// One image in the preload cache. The texture is shared with [`Showing`], so evicting the image
/// on screen does not free it mid-display.
struct Cached {
    texture: Rc<ImageTexture>,
    /// In the file; the texture is smaller when it was downscaled to the GPU's limit.
    original_size: [u32; 2],
    orientation: Orientation,
}

/// What is on screen. It changes only once the current image is ready, so an image change never
/// shows a blank frame.
enum Showing {
    Image {
        /// `None` for the stand-in `SUPERSLIDE_STAND_IN` puts up at startup, and for the
        /// clipboard image.
        path: Option<PathBuf>,
        texture: Rc<ImageTexture>,
        original_size: [u32; 2],
        view: View,
    },
    Error {
        path: PathBuf,
        file_name: String,
        reason: String,
    },
    /// No image yet, or the last image was deleted or moved away. The welcome screen draws over
    /// it while no image list is open or opening.
    Nothing,
}

impl Showing {
    fn path(&self) -> Option<&Path> {
        match self {
            Showing::Image { path, .. } => path.as_deref(),
            Showing::Error { path, .. } => Some(path),
            Showing::Nothing => None,
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
    preferences: Preferences,
    /// `None` only when the executable's own path is unknown; nothing is saved then.
    prefs_file: Option<TomlFile<Preferences>>,
    /// Written only while *Session restore* is on; `None` as for `prefs_file`.
    session_file: Option<TomlFile<Session>>,
    /// Something the session holds has changed since it was last saved.
    session_changed: bool,
    /// The window moved or resized since `placement` was last read. Read once the events have
    /// settled: winit sends `Moved` for a maximise before it knows the window is maximised.
    window_moved: bool,
    session_saved_at: Option<Instant>,
    /// What to scan to rebuild the image list, for the session; set with the image list, or
    /// before it while the session's own list is being scanned again.
    opened: Option<Open>,
    /// The session's place in its image list while that list is being scanned again, so a
    /// session saved before the scan ends keeps it.
    resuming: Option<Resume>,
    /// Where the window last was while neither fullscreen, maximised nor minimised.
    placement: Option<Placement>,
    /// The preferences screen, while it is open. Like the help overlay, it takes all input
    /// before the keymap.
    preferences_screen: Option<Screen>,
    showing: Showing,
    images: Option<ImageList>,
    /// Which way the owner last moved; the preload window leans this way.
    direction: Direction,
    advance: AdvanceLimit,
    slideshow: Slideshow,
    /// Screen rotation per path, until the app closes; across opens too.
    turns: HashMap<PathBuf, u8>,
    cache: PreloadCache<Cached>,
    workers: Workers,
    file_actions: FileActions,
    /// Paths with a file action asked for but not finished. A second Delete pressed before the
    /// first one's result would act on the same file again.
    acting_on: HashSet<PathBuf>,
    /// Where M and C last sent an image; their folder picker opens there. Kept in the session.
    target_folder: Option<PathBuf>,
    clipboard: ClipboardThread,
    /// The clipboard image holds the screen: a decode finishing does not replace it, and the next
    /// navigation goes back to the image list's current image rather than stepping past it.
    pasted: bool,
    waiting: Option<Waiting>,
    /// A scan is running. The welcome screen stays away meanwhile, so a session restore or a
    /// command-line open never flashes it before the first image.
    scanning: bool,
    welcome_logo: egui::TextureHandle,
    /// The help overlay, while it is open. It takes all input before the keymap.
    help: Option<Help>,
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
                    self.open(open, None);
                }
            }
            command => shown.run(command, event_loop),
        }
    }

    /// Starts a scan on its own thread, replacing (and so cancelling) any scan still running.
    /// `resume` restores a session's order and current image.
    fn open(&mut self, open: Open, resume: Option<Resume>) {
        self.scan_generation += 1;
        let generation = self.scan_generation;
        let proxy = self.proxy.clone();
        // Opens start only once the window exists, and with it the preferences.
        let Some(shown) = &mut self.shown else { return };
        shown.scanning = true;
        self.scan = Some(Scan::spawn(
            open,
            &shown.preferences.folder_filter,
            resume,
            move |event| {
                // Fails only once the event loop has exited, when nobody wants the result.
                let _ = proxy.send_event(AppEvent::Scan { generation, event });
            },
        ));
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
                shown.scanning = false;
                let Found {
                    list,
                    opened,
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
                shown.opened(list, opened, now);
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
        let (prefs_file, preferences, prefs_problem) = TomlFile::<Preferences>::beside_exe();
        // Off, the session file is neither read nor written, so it keeps no record of what was
        // viewed; turned on later, the first save replaces whatever an earlier run left there.
        let (session_file, session, session_problem) = match preferences.session_restore {
            true => TomlFile::<Session>::beside_exe(),
            false => (TomlFile::beside_exe_unread(), Session::default(), None),
        };
        let monitors: Vec<Monitor> = event_loop.available_monitors().map(monitor).collect();
        let primary = event_loop.primary_monitor().map(monitor);
        let restored = session
            .window
            .as_ref()
            .and_then(|saved| saved.restored(&monitors, primary.as_ref()));
        let mut attributes = Window::default_attributes().with_title(TITLE);
        if let Some(placement) = &restored {
            let [x, y] = placement.position;
            let [width, height] = placement.size;
            attributes = attributes
                .with_position(PhysicalPosition::new(x, y))
                .with_inner_size(PhysicalSize::new(width, height))
                .with_maximized(placement.maximized);
        }
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(e) => return fail(event_loop, &format!("Cannot open a window: {e}")),
        };
        set_icons(&window);
        // Read before fullscreen covers it; a maximised window's place is the restored one.
        let placement = restored.clone().or_else(|| plain_placement(&window));
        // The same call F11 makes, on the monitor the window is on. A session left fullscreen
        // comes back fullscreen; otherwise the preference decides.
        if preferences.start_fullscreen || restored.is_some_and(|placement| placement.fullscreen) {
            window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
        }
        // A command-line path is a request for that image: the session's image list gives way,
        // though its window placement and target folder still apply.
        let restoring = match self.open_at_start {
            Some(_) => None,
            None => session
                .images
                .as_ref()
                .map(|images| (open_of(images), images.resume())),
        };
        let mut renderer = match Renderer::new(window.clone()) {
            Ok(renderer) => renderer,
            Err(e) => return fail(event_loop, &format!("Cannot start drawing: {e}")),
        };
        let size = window.inner_size();
        let window_size = [size.width.max(1), size.height.max(1)];
        let showing = match render::stand_in() {
            Some((image_size, pixels)) => {
                let texture = Rc::new(renderer.upload(image_size, &pixels));
                let view = View::new(texture.size(), Orientation::Normal, 0, window_size);
                Showing::Image {
                    path: None,
                    texture,
                    original_size: image_size,
                    view,
                }
            }
            None => Showing::Nothing,
        };
        let proxy = self.proxy.clone();
        let workers = Workers::spawn(renderer.max_texture_dimension, move |path, result| {
            // Fails only once the event loop has exited, when nobody wants the result.
            let _ = proxy.send_event(AppEvent::Decoded { path, result });
        });
        let proxy = self.proxy.clone();
        let file_actions = FileActions::spawn(move |path, action, result| {
            // Fails only once the event loop has exited, when nobody wants the result.
            let _ = proxy.send_event(AppEvent::Acted {
                path,
                action,
                result,
            });
        });
        let proxy = self.proxy.clone();
        let loading = Loading {
            decode,
            shrink_to_fit,
            max_texture_dimension: renderer.max_texture_dimension,
        };
        let clipboard = ClipboardThread::spawn(loading, move |done| {
            // Fails only once the event loop has exited, when nobody wants the result.
            let _ = proxy.send_event(AppEvent::Clipboard(done));
        });
        renderer.magnification_filter = preferences.magnification_filter;
        let budget = u64::from(preferences.preload_budget_mb) << 20;
        let advance_limit = Duration::from_millis(preferences.advance_limit_ms.into());
        let slideshow_interval = Duration::from_secs(preferences.slideshow_interval_s.into());
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
            preferences,
            prefs_file,
            session_file,
            session_changed: false,
            window_moved: false,
            session_saved_at: None,
            opened: restoring.as_ref().map(|(open, _)| open.clone()),
            resuming: restoring.as_ref().map(|(_, resume)| resume.clone()),
            placement,
            preferences_screen: None,
            showing,
            images: None,
            direction: Direction::Forward,
            advance: AdvanceLimit::new(advance_limit),
            slideshow: Slideshow::new(slideshow_interval),
            turns: HashMap::new(),
            cache: PreloadCache::new(budget),
            workers,
            file_actions,
            acting_on: HashSet::new(),
            target_folder: session.target_folder,
            clipboard,
            pasted: false,
            waiting: None,
            scanning: false,
            welcome_logo: welcome::load_logo(&egui),
            help: None,
            egui,
            egui_input,
            notices: Notices::default(),
            egui_repaint_at: None,
            mouse: keymap::Mouse::default(),
            modifiers: ModifiersState::empty(),
            cursor: [0.0, 0.0],
        });
        if let Some(shown) = &mut self.shown {
            let now = Instant::now();
            if let Some(problem) = prefs_problem {
                shown
                    .notices
                    .notify(preferences::load_problem(&problem), now);
            }
            if let Some(problem) = session_problem {
                shown
                    .notices
                    .notify(preferences::session_problem(&problem), now);
            }
        }
        match restoring {
            Some((open, resume)) => self.open(open, Some(resume)),
            None => {
                if let Some(open) = self.open_at_start.take() {
                    self.open(open, None);
                }
            }
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
            AppEvent::Acted {
                path,
                action,
                result,
            } => {
                if let Some(shown) = &mut self.shown {
                    shown.acted(path, action, result);
                }
            }
            AppEvent::Clipboard(done) => {
                if let Some(shown) = &mut self.shown {
                    shown.clipboard_done(done);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(shown) = &mut self.shown else { return };
        // egui sees every event, so its pointer and modifier state stay current, but takes one only
        // while a widget has focus; otherwise all input belongs to the help overlay, when open,
        // then the keymap.
        let response = shown.egui_input.on_window_event(&shown.window, &event);
        let egui_keeps =
            response.consumed && shown.egui.memory(|memory| memory.focused().is_some());
        if shown.help.is_some() && shown.help_input(&event, egui_keeps) {
            shown.window.request_redraw();
            return;
        }
        if shown.preferences_screen.is_some() && shown.preferences_input(&event, egui_keeps) {
            shown.window.request_redraw();
            return;
        }
        if egui_keeps {
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
                shown.window_moved = true;
                shown.window.request_redraw();
            }
            WindowEvent::Moved(_) => shown.window_moved = true,
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

    /// A change made just before closing, such as with the preferences screen still open, is not
    /// lost. Too late for a notice if a save fails.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(shown) = &mut self.shown {
            shown.save_preferences();
            shown.read_placement();
            shown.save_session();
        }
    }

    /// Opens a burst of drops and saves a changed session. Sleeps until a notice starts fading,
    /// `Loading…` is due, a held key may advance, the slideshow interval ends, egui asks for a
    /// frame or a changed session may be saved; redraws every frame while a notice fades. A slideshow waiting on a decode needs no wake-up: the decode result
    /// is an event, and this runs again after it.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.dropped.is_empty() {
            let paths = std::mem::take(&mut self.dropped);
            self.open(Open::Paths(paths), None);
        }
        let Some(shown) = &mut self.shown else { return };
        let now = Instant::now();
        shown.read_placement();
        let session_at = shown.save_session_when_due(now);
        if let Some(direction) = shown.advance.tick(now) {
            shown.navigate(direction);
        }
        if shown.slideshow.tick(now, shown.next_ready()) {
            shown.navigate(Direction::Forward);
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
            shown.slideshow.due().filter(|&due| due > now),
            session_at,
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
    /// Input while the help overlay is open. Every press, click and wheel turn stops here, so
    /// none reaches a binding underneath; true when the event was taken. Key releases pass on, so
    /// a Space held before the overlay opened still stops repeating.
    fn help_input(&mut self, event: &WindowEvent, egui_keeps: bool) -> bool {
        let Some(open) = &mut self.help else {
            return false;
        };
        match event {
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // The filter box has the key; egui types it.
                if egui_keeps {
                    return false;
                }
                match keymap::for_help_key(&event.logical_key, self.modifiers, event.repeat) {
                    Some(HelpKey::Close) => self.help = None,
                    Some(HelpKey::FocusFilter) => open.focus_filter = true,
                    Some(HelpKey::Scroll(by)) => open.scroll(by),
                    None => {}
                }
                true
            }
            // Clicks on the panel reach egui, which has already seen them; on the image they
            // do nothing.
            WindowEvent::MouseInput { .. } => true,
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(by) = keymap::for_help_wheel(*delta) {
                    open.scroll(by);
                }
                true
            }
            _ => false,
        }
    }

    /// Input while the preferences screen is open, routed as for the help overlay: every press,
    /// click and wheel turn stops here, and egui, which has already seen them, drives the screen.
    fn preferences_input(&mut self, event: &WindowEvent, egui_keeps: bool) -> bool {
        match event {
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // A field has the key; egui types it, and Esc only leaves the field.
                if egui_keeps {
                    return false;
                }
                if keymap::closes_preferences(&event.logical_key, self.modifiers, event.repeat) {
                    self.preferences_screen = None;
                    self.save_preferences();
                }
                true
            }
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } => true,
            _ => false,
        }
    }

    /// Brings everything that reads a preference in line with `self.preferences`. The folder
    /// filter needs nothing: the next open reads it.
    fn apply_preferences(&mut self) {
        let preferences = &self.preferences;
        self.advance
            .set_limit(Duration::from_millis(preferences.advance_limit_ms.into()));
        self.slideshow
            .set_interval(Duration::from_secs(preferences.slideshow_interval_s.into()));
        self.cache
            .set_budget(u64::from(preferences.preload_budget_mb) << 20);
        self.renderer.magnification_filter = preferences.magnification_filter;
    }

    /// Writes the preferences file if they changed. Only a run's first failure gives a notice.
    fn save_preferences(&mut self) {
        let Some(file) = &mut self.prefs_file else {
            return;
        };
        if let Some(error) = file.save(&self.preferences) {
            self.notices
                .notify(preferences::save_failed(&error), Instant::now());
            self.window.request_redraw();
        }
    }

    /// After a move or resize: a plain window's place is kept; fullscreen or maximised, only
    /// that state is, so leaving it returns to the place. Minimised, Windows reports nothing
    /// useful.
    fn read_placement(&mut self) {
        if !std::mem::take(&mut self.window_moved) {
            return;
        }
        self.session_changed = true;
        let window = &self.window;
        if window.is_minimized() == Some(true) {
            return;
        }
        match (plain_placement(window), &mut self.placement) {
            (Some(plain), _) => self.placement = Some(plain),
            (None, Some(placement)) => {
                placement.fullscreen = window.fullscreen().is_some();
                placement.maximized = window.is_maximized();
            }
            // Never yet plain, so there is no place to go back to.
            (None, None) => {}
        }
    }

    fn session(&self) -> Session {
        let images = self.opened.as_ref().and_then(|open| {
            let resume = match &self.images {
                Some(images) => Resume {
                    order: images.order(),
                    current: images.current().map(|entry| entry.path.clone()),
                    position: images.current_position(),
                },
                None => self.resuming.clone()?,
            };
            Some(session_images(open, resume))
        });
        Session {
            target_folder: self.target_folder.clone(),
            images,
            window: self.placement.clone(),
        }
    }

    /// Writes the session file if the session changed, while *Session restore* is on. Only a
    /// run's first failure gives a notice.
    fn save_session(&mut self) {
        self.session_changed = false;
        self.session_saved_at = Some(Instant::now());
        if !self.preferences.session_restore {
            return;
        }
        let session = self.session();
        let Some(file) = &mut self.session_file else {
            return;
        };
        if let Some(error) = file.save(&session) {
            self.notices
                .notify(preferences::session_save_failed(&error), Instant::now());
            self.window.request_redraw();
        }
    }

    /// Saves a changed session once `SESSION_INTERVAL` has passed since the last save; until
    /// then, returns when it will have.
    fn save_session_when_due(&mut self, now: Instant) -> Option<Instant> {
        if !self.session_changed {
            return None;
        }
        let due = self
            .session_saved_at
            .map_or(now, |at| at + SESSION_INTERVAL);
        if now < due {
            return Some(due);
        }
        self.save_session();
        None
    }

    fn view(&mut self) -> Option<&mut View> {
        match &mut self.showing {
            Showing::Image { view, .. } => Some(view),
            Showing::Error { .. } | Showing::Nothing => None,
        }
    }

    /// A finished scan replaces the image list. Cached images it still holds are kept, at their
    /// new positions. An open that found nothing keeps the old list: its notice says so, and the
    /// owner carries on where they were.
    fn opened(&mut self, list: ImageList, opened: Open, now: Instant) {
        if list.len() == 0 {
            return;
        }
        self.opened = Some(opened);
        self.resuming = None;
        self.session_changed = true;
        if self.slideshow.stop() {
            self.notices
                .notify_as(SLIDESHOW_NOTICE, "Slideshow off", now);
        }
        self.cache
            .reindex(list.entries().iter().map(|entry| entry.path.as_path()));
        self.images = Some(list);
        // An open is explicit, so it replaces the clipboard image too.
        self.pasted = false;
        self.direction = Direction::Forward;
        self.retarget();
        self.show_current(now);
    }

    fn navigate(&mut self, direction: Direction) {
        let Some(images) = &mut self.images else {
            return;
        };
        if self.pasted {
            return self.leave_clipboard(direction);
        }
        let wrapped = images.step(direction);
        self.moved(direction, wrapped);
    }

    /// Does nothing unless grouped by subfolder.
    fn jump_group(&mut self, direction: Direction) {
        if self.pasted && self.images.is_some() {
            return self.leave_clipboard(direction);
        }
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
    /// Every change, manual or not, gives the slideshow a full interval.
    fn moved(&mut self, direction: Direction, wrapped: bool) {
        let now = Instant::now();
        self.session_changed = true;
        self.slideshow.restart(now);
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

    /// Back to the image list's current image, the one the clipboard image covered.
    fn leave_clipboard(&mut self, direction: Direction) {
        self.pasted = false;
        self.moved(direction, false);
    }

    /// Re-sorts in memory; the current image stays current, so nothing on screen changes but the
    /// notice. The preload window moves to the current image's new neighbours.
    fn reorder(&mut self, change: impl FnOnce(Order) -> Order) {
        let Some(images) = &mut self.images else {
            return;
        };
        let order = change(images.order());
        images.set_order(order);
        self.session_changed = true;
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

    /// Whether the slideshow's next image is decoded, or failed to; either can go on screen at
    /// once. A list of one has no next image.
    fn next_ready(&self) -> bool {
        self.images
            .as_ref()
            .filter(|images| images.len() > 1)
            .and_then(|images| images.adjacent(Direction::Forward))
            .is_some_and(|entry| self.cache.get(&entry.path).is_some())
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
        if self.pasted {
            return;
        }
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
                original_size: cached.original_size,
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
                    original_size: decoded.original_size,
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

    /// Asks the file action thread to delete (`None`), move or copy the image on screen: the one
    /// the owner sees, even while the current image is still loading.
    fn act(&mut self, transfer: Option<Transfer>) {
        let path = match &self.showing {
            Showing::Nothing => return,
            showing => showing.path().map(Path::to_path_buf),
        };
        // The stand-in and the clipboard image exist only in memory.
        let Some(path) = path else {
            self.notices.notify(NOT_A_FILE, Instant::now());
            return self.window.request_redraw();
        };
        if self.acting_on.contains(&path) {
            return;
        }
        let action = match transfer {
            None => Action::Delete,
            Some(transfer) => {
                let Some(folder) = self.pick_folder(transfer, &path) else {
                    return;
                };
                self.target_folder = Some(folder.clone());
                self.session_changed = true;
                Action::Transfer(transfer, folder)
            }
        };
        self.acting_on.insert(path.clone());
        self.file_actions.request(path, action);
    }

    /// The native folder picker for M or C, opening where the last one went, else at the
    /// image's own folder. Modal, like [`Self::pick`]. `None` when cancelled.
    fn pick_folder(&self, transfer: Transfer, path: &Path) -> Option<PathBuf> {
        let title = match transfer {
            Transfer::Move => "Move to folder",
            Transfer::Copy => "Copy to folder",
        };
        rfd::FileDialog::new()
            .set_parent(&*self.window)
            .set_title(title)
            .set_directory(self.target_folder.as_deref().unwrap_or(folder(path)))
            .pick_folder()
    }

    /// Every outcome gets a notice; a deleted or moved image leaves the image list.
    fn acted(&mut self, path: PathBuf, action: Action, result: Result<(), Failure>) {
        self.acting_on.remove(&path);
        let now = Instant::now();
        let text = file_actions::describe(&file_name(&path), &action, &result);
        self.notices.notify(text, now);
        self.window.request_redraw();
        if result.is_ok() && action.removes() {
            self.remove(&path, now);
        }
    }

    /// Takes a deleted or moved image out of the image list and the preload cache. If it was
    /// current, the next image shows, at once when it was preloaded.
    fn remove(&mut self, path: &Path, now: Instant) {
        let Some(images) = &mut self.images else {
            return;
        };
        let Some(removed) = images.remove(path) else {
            return;
        };
        // The current image's position may have moved.
        self.session_changed = true;
        self.turns.remove(path);
        self.cache
            .reindex(images.entries().iter().map(|entry| entry.path.as_path()));
        if images.len() == 0 {
            self.images = None;
            if self.slideshow.stop() {
                self.notices
                    .notify_as(SLIDESHOW_NOTICE, "Slideshow off", now);
            }
            self.waiting = None;
            self.notices.dismiss(LOADING_NOTICE);
            self.notices.notify("No images left", now);
            // The clipboard image stays: it was never in the image list.
            if !self.pasted {
                self.showing = Showing::Nothing;
                self.window.set_title(TITLE);
            }
            return;
        }
        if removed.was_current {
            // The next image is the one after, so travel is forward from here.
            self.moved(Direction::Forward, removed.wrapped);
        } else {
            self.retarget();
        }
    }

    /// Ctrl+C: the image on screen, turned as shown, onto the clipboard. An error card's file is
    /// tried again, so its notice says why it cannot be copied.
    fn copy(&mut self) {
        let now = Instant::now();
        let (path, turns) = match &self.showing {
            Showing::Nothing => return,
            Showing::Image { .. } if self.pasted => {
                self.notices.notify_as(
                    CLIPBOARD_NOTICE,
                    "This image is already on the clipboard",
                    now,
                );
                return self.window.request_redraw();
            }
            Showing::Image { path: None, .. } => {
                self.notices.notify(NOT_A_FILE, now);
                return self.window.request_redraw();
            }
            Showing::Image {
                path: Some(path),
                view,
                ..
            } => (path.clone(), view.turns()),
            Showing::Error { path, .. } => {
                (path.clone(), self.turns.get(path).copied().unwrap_or(0))
            }
        };
        let text = format!("Copying {}…", file_name(&path));
        self.notices.notify_as(CLIPBOARD_NOTICE, text, now);
        self.window.request_redraw();
        self.clipboard.request(Request::Copy { path, turns });
    }

    fn clipboard_done(&mut self, done: Done) {
        let now = Instant::now();
        self.window.request_redraw();
        let decoded = match done {
            Done::Copied { path, result } => {
                let text = clipboard::copied(&file_name(&path), &result);
                return self.notices.notify_as(CLIPBOARD_NOTICE, text, now);
            }
            Done::Pasted(Err(failure)) => {
                let text = clipboard::paste_failed(&failure);
                return self.notices.notify_as(CLIPBOARD_NOTICE, text, now);
            }
            Done::Pasted(Ok(decoded)) => decoded,
        };
        let texture = Rc::new(self.renderer.upload(decoded.size, &decoded.pixels));
        self.showing = Showing::Image {
            path: None,
            view: View::new(texture.size(), Orientation::Normal, 0, self.window_size),
            texture,
            original_size: decoded.original_size,
        };
        self.pasted = true;
        // Covers any image still loading; leaving the clipboard image starts the wait afresh.
        self.waiting = None;
        self.notices.dismiss(LOADING_NOTICE);
        self.notices
            .notify_as(CLIPBOARD_NOTICE, "Clipboard image", now);
        self.window.set_title(&format!("Clipboard image — {TITLE}"));
        self.slideshow.restart(now);
    }

    /// What the info overlay describes: the image on screen, which is not the current image
    /// while the next one loads.
    fn details(&self) -> Option<Details> {
        let (path, pixels) = match &self.showing {
            Showing::Nothing => return None,
            Showing::Image {
                path,
                texture,
                original_size,
                view,
            } => {
                let pixels = Pixels {
                    original: *original_size,
                    displayed: texture.size(),
                    scale: view.scale(),
                    nearest: self.preferences.magnification_filter == MagnificationFilter::Nearest,
                };
                (path.as_deref(), Some(pixels))
            }
            Showing::Error { path, .. } => (Some(path.as_path()), None),
        };
        let subject = match path {
            None if self.pasted => Subject::Clipboard,
            None => Subject::StandIn,
            Some(path) => Subject::File(FileFacts {
                name: file_name(path),
                folder: folder(path).display().to_string(),
                listed: self.listed(path),
            }),
        };
        Some(Details { subject, pixels })
    }

    fn listed(&self, path: &Path) -> Option<Listed> {
        let images = self.images.as_ref()?;
        // Nearly always the current image; any other costs a search of the list.
        let position = match images.current() {
            Some(entry) if entry.path == path => images.current_position(),
            _ => images.position(path)?,
        };
        let entry = &images.entries()[position];
        Some(Listed {
            position: position + 1,
            count: images.len(),
            bytes: entry.size,
            modified: local_time(entry.modified),
            order: describe(images.order()),
        })
    }

    /// Nothing on screen and nothing on its way: the window would otherwise be blank.
    fn welcoming(&self) -> bool {
        matches!(self.showing, Showing::Nothing) && self.images.is_none() && !self.scanning
    }

    fn redraw(&mut self) {
        let now = Instant::now();
        let input = self.egui_input.take_egui_input(&self.window);
        let info_mode = self.preferences.info_overlay;
        let details = match info_mode {
            InfoOverlay::Hidden => None,
            _ => self.details(),
        };
        let showing = &self.showing;
        let welcoming = self.welcoming().then_some(&self.welcome_logo);
        let shown_notices = &mut self.notices;
        let open_help = &mut self.help;
        let before = self
            .preferences_screen
            .is_some()
            .then(|| self.preferences.clone());
        let edited = &mut self.preferences;
        let screen = &mut self.preferences_screen;
        let output = self.egui.run_ui(input, |ui| {
            if let Some(logo) = welcoming {
                welcome::show(ui.ctx(), logo);
            }
            if let Showing::Error {
                file_name, reason, ..
            } = showing
            {
                error_card(ui.ctx(), file_name, reason);
            }
            if let Some(details) = &details {
                info_overlay::show(ui.ctx(), info_mode, details);
            }
            if let Some(open) = open_help {
                let rows: Vec<Binding> = keymap::BINDINGS.iter().map(|row| row.binding).collect();
                help::show(ui.ctx(), open, &rows);
            }
            if let Some(screen) = screen {
                preferences::show(ui.ctx(), edited, screen);
            }
            notices::show(shown_notices, ui.ctx(), now);
        });
        // Applied before drawing, so a filter change shows on this frame. The info overlay's
        // details were gathered before the screen ran, so the redraw catches it up.
        if before.is_some_and(|before| before != self.preferences) {
            self.apply_preferences();
            self.window.request_redraw();
        }
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
            Showing::Error { .. } | Showing::Nothing => None,
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
            Command::DeleteToRecycleBin => return self.act(None),
            Command::Transfer(transfer) => return self.act(Some(transfer)),
            Command::CopyToClipboard => return self.copy(),
            Command::ToggleHelp => {
                self.help = Some(Help::new());
                // A button held as it opened must not click or drag once it closes.
                self.mouse = keymap::Mouse::default();
            }
            Command::PasteFromClipboard => return self.clipboard.request(Request::Paste),
            // Without an image list there is nothing to show; opening one would stop it anyway.
            Command::ToggleSlideshow if self.images.is_some() => {
                let now = Instant::now();
                let text = match self.slideshow.toggle(now) {
                    true => "Slideshow on",
                    false => "Slideshow off",
                };
                self.notices.notify_as(SLIDESHOW_NOTICE, text, now);
            }
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
            Command::TogglePreferences => {
                self.preferences_screen = Some(Screen::new(&self.preferences));
                // A button held as it opened must not click or drag once it closes.
                self.mouse = keymap::Mouse::default();
            }
            // Both are preferences, so each press is saved, as the screen's changes are.
            Command::CycleInfoOverlay => {
                let mode = &mut self.preferences.info_overlay;
                *mode = mode.cycled();
                self.save_preferences();
            }
            Command::ToggleMagnificationFilter => {
                let filter = &mut self.preferences.magnification_filter;
                *filter = filter.toggled();
                self.apply_preferences();
                self.save_preferences();
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

/// The scan that rebuilds a saved image list. `Session::validated` leaves a recursive open
/// exactly one path.
fn open_of(images: &session::Images) -> Open {
    match images.route {
        Route::Paths => Open::Paths(images.paths.clone()),
        Route::Recursive => Open::Recursive(images.paths[0].clone()),
    }
}

fn session_images(open: &Open, resume: Resume) -> session::Images {
    let (route, paths) = match open {
        Open::Paths(paths) => (Route::Paths, paths.clone()),
        Open::Recursive(root) => (Route::Recursive, vec![root.clone()]),
    };
    session::Images {
        route,
        paths,
        current: resume.current,
        position: resume.position,
        order: resume.order,
    }
}

fn monitor(handle: MonitorHandle) -> Monitor {
    let position = handle.position();
    let size = handle.size();
    Monitor {
        name: handle.name(),
        position: [position.x, position.y],
        size: [size.width, size.height],
    }
}

/// The window's place while it is neither fullscreen nor maximised, which report the monitor's
/// area instead.
fn plain_placement(window: &Window) -> Option<Placement> {
    if window.fullscreen().is_some() || window.is_maximized() {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size();
    Some(Placement {
        position: [position.x, position.y],
        size: [size.width, size.height],
        monitor: window.current_monitor().and_then(|monitor| monitor.name()),
        maximized: false,
        fullscreen: false,
    })
}

/// Startup failures come before any overlay exists, and release builds have no console, so a
/// message box is the only way the owner hears about them.
/// The title bar and taskbar take the executable's own icon, each loaded at its size for the
/// window's DPI, so neither is a blurred rescale of the other.
fn set_icons(window: &Window) {
    let icon = |points: f64| {
        let side = (points * window.scale_factor()).round() as u32;
        Icon::from_resource(ICON_RESOURCE, Some(PhysicalSize::new(side, side)))
            .expect("build.rs embeds the icon in the executable")
    };
    window.set_window_icon(Some(icon(16.0)));
    window.set_taskbar_icon(Some(icon(32.0)));
}

fn fail(event_loop: &ActiveEventLoop, message: &str) {
    eprintln!("superslide: {message}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("SuperSlide-RS")
        .set_description(message)
        .show();
    event_loop.exit();
}
