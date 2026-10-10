/// One line of the keymap: the keys as the owner reads them and what they do. `shell` owns the
/// table and hands rows in, so this slice never sees `Command` or winit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "scaffold: the help overlay (SSR-22) reads it")
)]
pub struct Binding {
    /// As shown, such as `Space, right click` or `Ctrl+C / Ctrl+V`.
    pub keys: &'static str,
    pub action: &'static str,
    pub section: Section,
}

/// The overlay's headings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Navigate,
    View,
    Order,
    Files,
    Clipboard,
    Window,
}
