//! What the info overlay says, worded from facts `shell` gathers each frame. No egui and no clock,
//! so every line is tested here.

use serde::{Deserialize, Serialize};

/// What the info overlay shows; I cycles through these in order. A preference, so it is kept
/// across runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfoOverlay {
    #[default]
    Hidden,
    FileName,
    Full,
}

impl InfoOverlay {
    pub fn cycled(self) -> Self {
        match self {
            Self::Hidden => Self::FileName,
            Self::FileName => Self::Full,
            Self::Full => Self::Hidden,
        }
    }
}

/// What is on screen. An error card has a file but no pixels; the clipboard image and the
/// stand-in have pixels but no file.
#[derive(Clone, Debug, PartialEq)]
pub struct Details {
    pub subject: Subject,
    pub pixels: Option<Pixels>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    File(FileFacts),
    Clipboard,
    /// The generated image shown before the first open.
    StandIn,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileFacts {
    pub name: String,
    pub folder: String,
    /// The image list's facts; `None` once the file has left the list, as a deleted image on
    /// screen while the next one loads has.
    pub listed: Option<Listed>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    /// From 1.
    pub position: usize,
    pub count: usize,
    pub bytes: u64,
    /// `None` for a time the system cannot show as a local date.
    pub modified: Option<LocalTime>,
    /// The order's wording, as its notice gives it (`browse::presentation::describe`).
    pub order: String,
}

/// A wall-clock minute in the owner's time zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pixels {
    /// In the file, or on the clipboard.
    pub original: [u32; 2],
    /// On the GPU: smaller than `original` when it was downscaled to the texture limit.
    pub displayed: [u32; 2],
    /// Screen pixels per displayed pixel.
    pub scale: f32,
    pub nearest: bool,
}

impl Pixels {
    /// Screen pixels per original pixel, as a percentage, so 100 % is the file's own pixels even
    /// when the GPU holds fewer. The long side carries the downscale exactly; the short side was
    /// rounded.
    pub fn zoom_percent(&self) -> f32 {
        let long = |size: [u32; 2]| size[0].max(size[1]) as f32;
        self.scale * long(self.displayed) / long(self.original) * 100.0
    }
}

/// The overlay's lines, title first; empty when hidden.
pub fn lines(mode: InfoOverlay, details: &Details) -> Vec<String> {
    let title = match &details.subject {
        Subject::File(file) => file.name.clone(),
        Subject::Clipboard => "Clipboard image".to_owned(),
        Subject::StandIn => "Test image".to_owned(),
    };
    let mut lines = match mode {
        InfoOverlay::Hidden => return Vec::new(),
        InfoOverlay::FileName => return vec![title],
        InfoOverlay::Full => vec![title],
    };
    let listed = match &details.subject {
        Subject::File(file) => file.listed.as_ref(),
        Subject::Clipboard | Subject::StandIn => None,
    };
    if let Some(listed) = listed {
        lines.push(format!("{} / {}", listed.position, listed.count));
    }
    if let Some(pixels) = &details.pixels {
        let mut size = dimensions(pixels.original);
        if pixels.displayed != pixels.original {
            size += &format!(", downscaled to {}", dimensions(pixels.displayed));
        }
        lines.push(size);
    }
    // The clipboard image and the stand-in show only their size: they have no file, no list
    // position, and nothing else of theirs is worth reading.
    let Subject::File(file) = &details.subject else {
        return lines;
    };
    if let Some(listed) = listed {
        lines.push(file_size(listed.bytes));
        if let Some(modified) = listed.modified {
            lines.push(format!("Modified {}", date(modified)));
        }
    }
    if let Some(pixels) = &details.pixels {
        lines.push(format!("Zoom {}", percent(pixels.zoom_percent())));
    }
    if let Some(listed) = listed {
        lines.push(listed.order.clone());
    }
    lines.push(file.folder.clone());
    if details.pixels.is_some_and(|pixels| pixels.nearest) {
        lines.push("Magnification: nearest".to_owned());
    }
    lines
}

fn dimensions([w, h]: [u32; 2]) -> String {
    format!("{w} × {h}")
}

/// Three significant figures, as Explorer's Properties gives them, in 1024s.
fn file_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let decimals = if value < 10.0 {
        2
    } else if value < 100.0 {
        1
    } else {
        0
    };
    format!("{value:.decimals$} {}", UNITS[unit])
}

/// Whole percent, but a tenth below 10 %, so a large image zoomed far out does not read 0 %.
fn percent(value: f32) -> String {
    if value < 10.0 {
        format!("{value:.1}%")
    } else {
        format!("{value:.0}%")
    }
}

/// Year first, so it reads the same in every locale.
fn date(t: LocalTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i_cycles_through_the_three_modes() {
        let mut mode = InfoOverlay::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(mode);
            mode = mode.cycled();
        }
        use InfoOverlay::*;
        assert_eq!(seen, [Hidden, FileName, Full, Hidden]);
    }

    fn listed() -> Listed {
        Listed {
            position: 7,
            count: 30,
            bytes: 19_608_576,
            modified: Some(LocalTime {
                year: 2026,
                month: 3,
                day: 9,
                hour: 8,
                minute: 5,
            }),
            order: "Sorted by date, reversed, grouped by folder".to_owned(),
        }
    }

    fn file(listed: Option<Listed>) -> Subject {
        Subject::File(FileFacts {
            name: "001.jpg".to_owned(),
            folder: r"D:\fixtures\50mp".to_owned(),
            listed,
        })
    }

    #[test]
    fn a_downscaled_jpeg_shows_both_sizes_and_zoom_against_the_file() {
        // 8660 × 5773 on a GPU capped at 8192: fit in a 1920 × 1080 window.
        let details = Details {
            subject: file(Some(listed())),
            pixels: Some(Pixels {
                original: [8660, 5773],
                displayed: [8192, 5461],
                scale: 1080.0 / 5461.0,
                nearest: false,
            }),
        };
        assert_eq!(
            lines(InfoOverlay::Full, &details),
            [
                "001.jpg",
                "7 / 30",
                "8660 × 5773, downscaled to 8192 × 5461",
                "18.7 MB",
                "Modified 2026-03-09 08:05",
                "Zoom 19%",
                "Sorted by date, reversed, grouped by folder",
                r"D:\fixtures\50mp",
            ]
        );
        assert_eq!(lines(InfoOverlay::FileName, &details), ["001.jpg"]);
        assert!(lines(InfoOverlay::Hidden, &details).is_empty());
    }

    #[test]
    fn a_small_png_shows_one_size_and_the_nearest_filter() {
        let details = Details {
            subject: file(Some(Listed {
                bytes: 700,
                ..listed()
            })),
            pixels: Some(Pixels {
                original: [64, 48],
                displayed: [64, 48],
                scale: 16.0,
                nearest: true,
            }),
        };
        let lines = lines(InfoOverlay::Full, &details);
        assert_eq!(lines[2..4], ["64 × 48", "700 bytes"]);
        assert_eq!(lines[5], "Zoom 1600%");
        assert_eq!(lines.last().unwrap(), "Magnification: nearest");
    }

    #[test]
    fn an_error_card_has_no_size_or_zoom() {
        let details = Details {
            subject: file(Some(listed())),
            pixels: None,
        };
        let lines = lines(InfoOverlay::Full, &details);
        assert_eq!(lines[1], "7 / 30");
        assert_eq!(lines[2], "18.7 MB");
        assert!(
            lines
                .iter()
                .all(|line| !line.contains('×') && !line.contains("Zoom"))
        );
    }

    #[test]
    fn a_file_gone_from_the_list_keeps_its_name_and_folder() {
        let details = Details {
            subject: file(None),
            pixels: None,
        };
        assert_eq!(
            lines(InfoOverlay::Full, &details),
            ["001.jpg", r"D:\fixtures\50mp"]
        );
    }

    #[test]
    fn the_clipboard_image_shows_its_name_and_size() {
        let details = Details {
            subject: Subject::Clipboard,
            pixels: Some(Pixels {
                original: [1920, 1080],
                displayed: [1920, 1080],
                scale: 0.5,
                nearest: true,
            }),
        };
        assert_eq!(
            lines(InfoOverlay::Full, &details),
            ["Clipboard image", "1920 × 1080"]
        );
    }

    #[test]
    fn file_sizes_keep_three_figures() {
        assert_eq!(file_size(0), "0 bytes");
        assert_eq!(file_size(1023), "1023 bytes");
        assert_eq!(file_size(1024), "1.00 KB");
        assert_eq!(file_size(15_360), "15.0 KB");
        assert_eq!(file_size(524_288), "512 KB");
        assert_eq!(file_size(5 << 30), "5.00 GB");
    }

    #[test]
    fn small_zooms_keep_a_decimal() {
        assert_eq!(percent(100.0), "100%");
        assert_eq!(percent(12.6), "13%");
        assert_eq!(percent(3.06), "3.1%");
    }
}
