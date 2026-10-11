//! What session restore brings back: the image list's opening, order and current image, the
//! order last chosen, the target folder for M and C, and the window's placement.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::browse::domain::{Order, Resume, SortKey, SortMode};

/// Smallest and largest inner size restored, in physical pixels; a hand-edited 0 × 0 window
/// could not be found to resize.
const WINDOW_SIZE: std::ops::RangeInclusive<u32> = 160..=32_768;
/// The strip along the window's top edge that must land on its monitor: it carries the title
/// bar, the handle for dragging a window back.
const TITLE_STRIP: i64 = 32;
const MIN_VISIBLE_WIDTH: i64 = 100;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
// A file from an older build that lacks a table still loads, without it.
#[serde(default)]
pub struct Session {
    /// Where M and C last sent an image.
    pub target_folder: Option<PathBuf>,
    pub images: Option<Images>,
    /// The order last chosen with a sort key, which new image lists take. Apart from the image
    /// list's own order, so it outlives the list it was chosen on.
    #[serde(
        default,
        with = "order::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub order: Option<Order>,
    pub window: Option<Placement>,
}

/// The image list: how it was opened, so it can be scanned again, and where it stood.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Images {
    pub route: Route,
    /// Absolute. Exactly one for a recursive open.
    pub paths: Vec<PathBuf>,
    #[serde(default)]
    pub current: Option<PathBuf>,
    #[serde(default)]
    pub position: usize,
    #[serde(with = "order")]
    pub order: Order,
}

impl Images {
    pub fn resume(&self) -> Resume {
        Resume {
            order: self.order,
            current: self.current.clone(),
            position: self.position,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Paths,
    Recursive,
}

/// The window as last placed while neither fullscreen, maximised nor minimised, and whether it
/// was then fullscreen or maximised; leaving either goes back to that place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// Outer top-left corner, in physical pixels on the virtual desktop.
    pub position: [i32; 2],
    /// Inner size in physical pixels, as winit sets and reports it.
    pub size: [u32; 2],
    /// The monitor holding the window, as Windows names it (`\\.\DISPLAY2`).
    #[serde(default)]
    pub monitor: Option<String>,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

/// A monitor's area, in physical pixels on the virtual desktop.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub name: Option<String>,
    pub position: [i32; 2],
    pub size: [u32; 2],
}

impl Session {
    /// The file is plain text the owner may edit by hand: a list that cannot be scanned as
    /// saved is dropped, and the window size is brought into range.
    pub fn validated(self) -> Self {
        let images = self.images.filter(|images| {
            let count_fits = match images.route {
                Route::Paths => !images.paths.is_empty(),
                Route::Recursive => images.paths.len() == 1,
            };
            // A relative path would resolve against whatever folder the next start runs in.
            count_fits && images.paths.iter().all(|path| path.is_absolute())
        });
        let window = self.window.map(|placement| Placement {
            size: placement
                .size
                .map(|side| side.clamp(*WINDOW_SIZE.start(), *WINDOW_SIZE.end())),
            ..placement
        });
        Self {
            images,
            window,
            ..self
        }
    }
}

impl Placement {
    /// Where the window opens: as saved while its monitor is still there and the window's top
    /// edge is on it; otherwise centred on the primary monitor. Either way no larger than that
    /// monitor. Keeps the maximised and fullscreen states; `None` when Windows reports no
    /// primary monitor, leaving the window where Windows puts it.
    pub fn restored(&self, monitors: &[Monitor], primary: Option<&Monitor>) -> Option<Self> {
        let saved_monitor = monitors.iter().find(|monitor| {
            monitor.name.is_some() && monitor.name == self.monitor && self.top_edge_on(monitor)
        });
        let (monitor, kept) = match saved_monitor {
            Some(monitor) => (monitor, true),
            None => (primary?, false),
        };
        let size = [
            self.size[0].min(monitor.size[0]),
            self.size[1].min(monitor.size[1]),
        ];
        let position = match kept {
            true => self.position,
            false => [0, 1].map(|axis| {
                let spare = i64::from(monitor.size[axis]) - i64::from(size[axis]);
                (i64::from(monitor.position[axis]) + spare / 2) as i32
            }),
        };
        Some(Self {
            position,
            size,
            monitor: monitor.name.clone(),
            ..self.clone()
        })
    }

    /// The title strip lies within the monitor's height and overlaps it by enough to drag.
    fn top_edge_on(&self, monitor: &Monitor) -> bool {
        let [x, y] = self.position.map(i64::from);
        let [left, top] = monitor.position.map(i64::from);
        let [width, height] = monitor.size.map(i64::from);
        let overlap = (x + i64::from(self.size[0])).min(left + width) - x.max(left);
        y >= top && y + TITLE_STRIP <= top + height && overlap >= MIN_VISIBLE_WIDTH
    }
}

/// `Order` as a TOML table: `sort` names the mode, and a shuffle's seed is hex, since TOML
/// integers stop at `i64::MAX` and a seed is any `u64`.
mod order {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    use super::{Order, SortKey, SortMode};

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Sort {
        Name,
        Date,
        Size,
        Shuffle,
    }

    #[derive(Serialize, Deserialize)]
    struct Fields {
        sort: Sort,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<String>,
        #[serde(default)]
        reversed: bool,
        #[serde(default)]
        grouped: bool,
    }

    pub fn serialize<S: Serializer>(order: &Order, serializer: S) -> Result<S::Ok, S::Error> {
        let (sort, seed) = match order.mode {
            SortMode::By(SortKey::Name) => (Sort::Name, None),
            SortMode::By(SortKey::Date) => (Sort::Date, None),
            SortMode::By(SortKey::Size) => (Sort::Size, None),
            SortMode::Shuffle { seed } => (Sort::Shuffle, Some(format!("{seed:016x}"))),
        };
        Fields {
            sort,
            seed,
            reversed: order.reversed,
            grouped: order.grouped,
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Order, D::Error> {
        let fields = Fields::deserialize(deserializer)?;
        let mode = match fields.sort {
            Sort::Name => SortMode::By(SortKey::Name),
            Sort::Date => SortMode::By(SortKey::Date),
            Sort::Size => SortMode::By(SortKey::Size),
            Sort::Shuffle => {
                let seed = fields.seed.as_deref().unwrap_or_default();
                SortMode::Shuffle {
                    seed: u64::from_str_radix(seed, 16)
                        .map_err(|_| D::Error::custom("a shuffle needs a hex seed"))?,
                }
            }
        };
        Ok(Order {
            mode,
            reversed: fields.reversed,
            grouped: fields.grouped,
        })
    }

    /// For an `Option<Order>` field, as the same table when present.
    pub mod option {
        use serde::{Deserialize, Deserializer, Serialize, Serializer};

        use super::Order;

        #[derive(Serialize, Deserialize)]
        struct Table(#[serde(with = "super")] Order);

        pub fn serialize<S: Serializer>(
            order: &Option<Order>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            order.map(Table).serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Order>, D::Error> {
            Ok(Option::<Table>::deserialize(deserializer)?.map(|Table(order)| order))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved() -> Session {
        Session {
            target_folder: Some(PathBuf::from(r"D:\Sorted")),
            images: Some(Images {
                route: Route::Recursive,
                paths: vec![PathBuf::from(r"C:\Pictures")],
                current: Some(PathBuf::from(r"C:\Pictures\b\c.jpg")),
                position: 41,
                order: Order {
                    // Above `i64::MAX`, which a TOML integer cannot hold.
                    mode: SortMode::Shuffle {
                        seed: 0xF00D_0000_0000_0001,
                    },
                    reversed: false,
                    grouped: true,
                },
            }),
            order: Some(Order {
                mode: SortMode::By(SortKey::Date),
                reversed: true,
                grouped: false,
            }),
            window: Some(Placement {
                position: [2600, -8],
                size: [1280, 720],
                monitor: Some(r"\\.\DISPLAY2".to_owned()),
                maximized: false,
                fullscreen: true,
            }),
        }
    }

    #[test]
    fn a_session_survives_a_toml_round_trip() {
        let text = toml::to_string(&saved()).unwrap();
        assert!(text.contains("seed = \"f00d000000000001\""), "{text}");
        assert_eq!(toml::from_str::<Session>(&text).unwrap(), saved());
        let empty = toml::to_string(&Session::default()).unwrap();
        assert_eq!(
            toml::from_str::<Session>(&empty).unwrap(),
            Session::default()
        );
    }

    #[test]
    fn a_session_without_a_chosen_order_loads_without_one() {
        let text = toml::to_string(&Session {
            order: None,
            ..saved()
        })
        .unwrap();
        assert!(!text.contains("[order]"), "{text}");
        let loaded = toml::from_str::<Session>(&text).unwrap();
        assert_eq!(loaded.order, None);
        assert_eq!(loaded.images, saved().images);
    }

    #[test]
    fn a_shuffle_without_a_readable_seed_does_not_load() {
        let text = toml::to_string(&saved()).unwrap();
        let bad = text.replace("f00d000000000001", "soon");
        assert!(toml::from_str::<Session>(&bad).is_err());
        let missing = text.replace("seed = \"f00d000000000001\"", "");
        assert!(toml::from_str::<Session>(&missing).is_err());
    }

    #[test]
    fn hand_edits_that_cannot_be_scanned_drop_the_list() {
        let with = |route, paths: &[&str]| {
            let mut session = saved();
            let images = session.images.as_mut().unwrap();
            images.route = route;
            images.paths = paths.iter().map(PathBuf::from).collect();
            session.validated().images.is_some()
        };
        assert!(with(Route::Recursive, &[r"C:\Pictures"]));
        assert!(with(Route::Paths, &[r"C:\a.jpg", r"C:\b"]));
        assert!(!with(Route::Paths, &[]));
        assert!(!with(Route::Recursive, &[r"C:\a", r"C:\b"]));
        assert!(!with(Route::Paths, &[r"C:\a.jpg", "relative"]));
        let mut tiny = saved();
        tiny.window.as_mut().unwrap().size = [0, 100_000];
        assert_eq!(tiny.validated().window.unwrap().size, [160, 32_768]);
        assert_eq!(saved().validated(), saved());
    }

    fn monitor(name: &str, x: i32, width: u32) -> Monitor {
        Monitor {
            name: Some(name.to_owned()),
            position: [x, 0],
            size: [width, 1440],
        }
    }

    fn placed(position: [i32; 2], size: [u32; 2], monitor: &str) -> Placement {
        Placement {
            position,
            size,
            monitor: Some(monitor.to_owned()),
            maximized: true,
            fullscreen: false,
        }
    }

    #[test]
    fn the_window_returns_to_its_monitor_while_that_is_there() {
        let primary = monitor(r"\\.\DISPLAY1", 0, 2560);
        let second = monitor(r"\\.\DISPLAY2", 2560, 2560);
        let monitors = [primary.clone(), second];
        let saved = placed([3000, 100], [1280, 720], r"\\.\DISPLAY2");
        assert_eq!(
            saved.restored(&monitors, Some(&primary)),
            Some(saved.clone())
        );

        // Unplugged: centred on the primary monitor, still maximised.
        let alone = saved.restored(&monitors[..1], Some(&primary)).unwrap();
        assert_eq!(alone, placed([640, 360], [1280, 720], r"\\.\DISPLAY1"));
    }

    #[test]
    fn a_window_off_its_monitor_or_too_big_is_centred_and_shrunk() {
        let primary = monitor(r"\\.\DISPLAY1", 0, 2560);
        let monitors = [primary.clone()];
        let centred = Some(placed([640, 360], [1280, 720], r"\\.\DISPLAY1"));
        for position in [
            // Title bar above the top, below the bottom, or under 100 px on screen.
            [100, -40],
            [100, 1420],
            [2470, 100],
            [-1200, 100],
        ] {
            let saved = placed(position, [1280, 720], r"\\.\DISPLAY1");
            assert_eq!(
                saved.restored(&monitors, Some(&primary)),
                centred,
                "{position:?}"
            );
        }
        // Exactly 100 px on screen is enough.
        let edge = placed([2460, 100], [1280, 720], r"\\.\DISPLAY1");
        assert_eq!(edge.restored(&monitors, Some(&primary)), Some(edge.clone()));

        let huge = placed([0, 0], [3840, 2160], r"\\.\DISPLAY1");
        let shrunk = huge.restored(&monitors, Some(&primary)).unwrap();
        assert_eq!((shrunk.position, shrunk.size), ([0, 0], [2560, 1440]));
        assert_eq!(huge.restored(&monitors, None), Some(shrunk.clone()));
        assert_eq!(
            placed([5000, 0], [800, 600], "x").restored(&monitors, None),
            None
        );
    }
}
