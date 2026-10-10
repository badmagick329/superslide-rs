//! The outer layer every slice plugs into: the winit event loop, input bindings and the GPU.
//! Slices never depend on it.

pub mod app;
pub mod command;
pub mod keymap;
pub mod render;
