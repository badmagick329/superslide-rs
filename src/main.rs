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

fn main() {}
