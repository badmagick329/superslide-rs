// Release builds are a GUI app: no console window behind the viewer. Debug builds keep it for logs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod cache;
mod clipboard;
mod decode;
mod files;
mod keymap;
mod library;
mod prefs;
mod render;
mod ui;
mod view;

use std::path::PathBuf;

use winit::event_loop::{ControlFlow, EventLoop};

fn main() {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let event_loop = EventLoop::<app::AppEvent>::with_user_event()
        .build()
        .expect("winit supports Windows");
    // Sleep until input, a redraw request or another thread's event; `App::about_to_wait` adds
    // wake-ups for notices.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = app::App::new(event_loop.create_proxy(), paths);
    event_loop
        .run_app(&mut app)
        .expect("the event loop runs until exit");
}
