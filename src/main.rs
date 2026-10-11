// Release builds are a GUI app: no console window behind the viewer. Debug builds keep it for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Vertical slices, each split into the layers it needs; `docs/context/architecture.md` *Shape*.
mod browse;
mod clipboard;
mod file_actions;
mod help;
mod image_file;
mod info_overlay;
mod load;
mod notices;
mod prefs;
mod shell;
mod view;
mod welcome;

use std::path::PathBuf;

use winit::event_loop::{ControlFlow, EventLoop};

use crate::shell::app::{App, AppEvent};

fn main() {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("winit supports Windows");
    // Sleep until input, a redraw request or another thread's event; `App::about_to_wait` adds
    // wake-ups for notices.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(event_loop.create_proxy(), paths);
    event_loop
        .run_app(&mut app)
        .expect("the event loop runs until exit");
}
