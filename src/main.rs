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

use std::ffi::OsString;
use std::path::PathBuf;

use winit::event_loop::{ControlFlow, EventLoop};

use crate::shell::app::{App, AppEvent};

/// What `--version` prints and the help overlay shows.
pub const NAME_AND_VERSION: &str = concat!("SuperSlide-RS ", env!("CARGO_PKG_VERSION"));

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if asks_version(&args) {
        print_version();
        return;
    }
    let paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
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

/// Only as the sole argument: anything else on the command line is a path to open.
fn asks_version(args: &[OsString]) -> bool {
    matches!(args, [flag] if flag == "--version" || flag == "-v")
}

fn print_version() {
    // A release build is a GUI app, started without a console; borrow the terminal that ran it.
    // With output redirected to a file or pipe, the inherited handle already works, and with no
    // parent console there is nowhere to print, so a failure here needs no handling.
    #[cfg(not(debug_assertions))]
    // SAFETY: takes no pointers; it only fails when there is no console to attach to.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
    println!("{NAME_AND_VERSION}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn either_version_flag_alone_asks_for_the_version() {
        assert!(asks_version(&args(&["--version"])));
        assert!(asks_version(&args(&["-v"])));
    }

    #[test]
    fn paths_and_flags_among_paths_open_images() {
        assert!(!asks_version(&args(&[])));
        assert!(!asks_version(&args(&["photo.jpg"])));
        assert!(!asks_version(&args(&["--version", "photo.jpg"])));
        assert!(!asks_version(&args(&["-V"])));
    }
}
