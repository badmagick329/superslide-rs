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

use winit::event_loop::{ControlFlow, EventLoop};

fn main() {
    let event_loop = EventLoop::new().expect("winit supports Windows");
    // Nothing animates yet: sleep until input or a redraw request arrives.
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop
        .run_app(&mut app::App::default())
        .expect("the event loop runs until exit");
}
