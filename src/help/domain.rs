/// One line of the keymap: the keys as the owner reads them and what they do. `shell` owns the
/// table and hands rows in, so this slice never sees `Command` or winit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// The rows whose keys or action contain `filter`, ignoring case and the filter's outer spaces,
/// in table order.
pub fn matching(rows: &[Binding], filter: &str) -> Vec<Binding> {
    let filter = filter.trim().to_lowercase();
    rows.iter()
        .filter(|row| {
            row.keys.to_lowercase().contains(&filter) || row.action.to_lowercase().contains(&filter)
        })
        .copied()
        .collect()
}

/// The help overlay while it is open. Each opening makes a new one, so it starts with an empty
/// filter, scrolled to the top.
#[derive(Debug)]
pub struct Help {
    pub filter: String,
    /// `/` was pressed; the filter box takes focus on the next frame.
    pub focus_filter: bool,
    /// The list as last drawn, in points; scrolls are measured against it.
    pub measured: Measured,
    /// Where a key or the wheel asked the list to scroll to since the last frame.
    scroll_to: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measured {
    pub offset: f32,
    pub max_offset: f32,
    /// The visible height of the list.
    pub page: f32,
    pub line: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scroll {
    /// Positive scrolls down.
    Lines(f32),
    Pages(f32),
    Top,
    Bottom,
}

impl Help {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            focus_filter: false,
            measured: Measured::default(),
            // The list's scroll position outlives the overlay in egui's memory.
            scroll_to: Some(0.0),
        }
    }

    /// Scrolls from where the last scroll asked, not where the last frame drew, so several wheel
    /// notches between two frames add up.
    pub fn scroll(&mut self, by: Scroll) {
        let m = self.measured;
        let from = self.scroll_to.unwrap_or(m.offset);
        let to = match by {
            Scroll::Lines(n) => from + n * m.line,
            // A page keeps one line of the last page in view, so reading can carry on.
            Scroll::Pages(n) => from + n * (m.page - m.line).max(m.line),
            Scroll::Top => 0.0,
            Scroll::Bottom => m.max_offset,
        };
        self.scroll_to = Some(to.clamp(0.0, m.max_offset));
    }

    pub fn take_scroll(&mut self) -> Option<f32> {
        self.scroll_to.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: [Binding; 3] = [
        Binding {
            keys: "Right drag, mouse wheel, + / -",
            action: "Zoom",
            section: Section::View,
        },
        Binding {
            keys: "Ctrl+ / Ctrl-",
            action: "Zoom by double / half",
            section: Section::View,
        },
        Binding {
            keys: "R / L",
            action: "Screen rotation right / left",
            section: Section::View,
        },
    ];

    #[test]
    fn the_filter_matches_keys_or_action_ignoring_case() {
        let keys = |filter| -> Vec<_> { matching(&ROWS, filter).iter().map(|r| r.keys).collect() };
        assert_eq!(
            keys("ZOOM"),
            ["Right drag, mouse wheel, + / -", "Ctrl+ / Ctrl-"]
        );
        assert_eq!(keys("ctrl"), ["Ctrl+ / Ctrl-"]);
        assert_eq!(keys(" rotation "), ["R / L"]);
        assert_eq!(keys("").len(), 3);
        assert!(keys("nothing like this").is_empty());
    }

    fn measured() -> Help {
        let mut help = Help::new();
        help.take_scroll();
        help.measured = Measured {
            offset: 100.0,
            max_offset: 500.0,
            page: 300.0,
            line: 20.0,
        };
        help
    }

    #[test]
    fn scrolls_stay_within_the_list() {
        let mut help = measured();
        help.scroll(Scroll::Lines(-10.0));
        assert_eq!(help.take_scroll(), Some(0.0));
        help.scroll(Scroll::Pages(1.0));
        assert_eq!(help.take_scroll(), Some(380.0));
        help.scroll(Scroll::Pages(2.0));
        assert_eq!(help.take_scroll(), Some(500.0));
        help.scroll(Scroll::Bottom);
        assert_eq!(help.take_scroll(), Some(500.0));
        help.scroll(Scroll::Top);
        assert_eq!(help.take_scroll(), Some(0.0));
        assert_eq!(help.take_scroll(), None, "a scroll applies once");
    }

    #[test]
    fn notches_between_frames_add_up() {
        let mut help = measured();
        help.scroll(Scroll::Lines(3.0));
        help.scroll(Scroll::Lines(3.0));
        assert_eq!(help.take_scroll(), Some(220.0));
    }

    #[test]
    fn a_new_overlay_starts_at_the_top() {
        assert_eq!(Help::new().take_scroll(), Some(0.0));
    }
}
