//! `superslide-prefs.toml` and `superslide-session.toml`, beside the executable so a copied
//! folder carries them.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::prefs::domain::Preferences;
use crate::prefs::domain::session::Session;

/// What one of the files holds.
pub trait Stored: Serialize + DeserializeOwned + Default + Clone + PartialEq {
    const FILE_NAME: &str;
    /// Brings hand-edited values into range.
    fn validated(self) -> Self;
}

impl Stored for Preferences {
    const FILE_NAME: &str = "superslide-prefs.toml";
    fn validated(self) -> Self {
        self.clamped()
    }
}

impl Stored for Session {
    const FILE_NAME: &str = "superslide-session.toml";
    fn validated(self) -> Self {
        Session::validated(self)
    }
}

/// Why the app runs on defaults.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadProblem {
    /// Not a file this build can read. It was renamed to `.bak`, so the next save does not
    /// destroy what the owner wrote.
    Damaged,
    /// Damaged, and the rename failed too; the next save replaces it.
    DamagedNotKept(String),
    Unreadable(String),
}

/// The file, and what it last held, so a save only writes a change.
#[derive(Debug)]
pub struct TomlFile<T> {
    path: PathBuf,
    /// `None` until read or written: the file may hold anything.
    saved: Option<T>,
    /// A save has failed this run: the owner has heard, once.
    failed: bool,
}

impl<T: Stored> TomlFile<T> {
    /// Beside the executable, read. A missing file is not a problem: the defaults are what it
    /// would hold.
    pub fn beside_exe() -> (Option<Self>, T, Option<LoadProblem>) {
        match std::env::current_exe() {
            Ok(exe) => {
                let (file, value, problem) = Self::load(exe.with_file_name(T::FILE_NAME));
                (Some(file), value, problem)
            }
            Err(e) => (
                None,
                T::default(),
                Some(LoadProblem::Unreadable(e.to_string())),
            ),
        }
    }

    /// Beside the executable, not read, so the first save writes whatever it is given. `None`
    /// when the executable's path is unknown.
    pub fn beside_exe_unread() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        Some(Self::unread(exe.with_file_name(T::FILE_NAME)))
    }

    fn unread(path: PathBuf) -> Self {
        Self {
            path,
            saved: None,
            failed: false,
        }
    }

    pub fn load(path: PathBuf) -> (Self, T, Option<LoadProblem>) {
        let (value, problem) = match fs::read(&path) {
            Ok(bytes) => match parse::<T>(&bytes) {
                Some(value) => (value, None),
                None => {
                    let problem = match fs::rename(&path, backup(&path)) {
                        Ok(()) => LoadProblem::Damaged,
                        Err(e) => LoadProblem::DamagedNotKept(e.to_string()),
                    };
                    (T::default(), Some(problem))
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => (T::default(), None),
            Err(e) => (T::default(), Some(LoadProblem::Unreadable(e.to_string()))),
        };
        let file = Self {
            path,
            saved: Some(value.clone()),
            failed: false,
        };
        (file, value, problem)
    }

    /// Writes `value` if it changed since the last write. Returns the error only for the run's
    /// first failure, so an unwritable folder (Program Files) gives one notice, not one per
    /// change; later saves still try, in case the folder became writable.
    pub fn save(&mut self, value: &T) -> Option<io::Error> {
        if self.saved.as_ref() == Some(value) {
            return None;
        }
        match write_atomically(&self.path, value) {
            Ok(()) => {
                self.saved = Some(value.clone());
                None
            }
            Err(e) => (!std::mem::replace(&mut self.failed, true)).then_some(e),
        }
    }
}

/// `None` for anything but a TOML table of known types: invalid UTF-8, bad syntax, or a field of
/// the wrong type. Unknown fields are ignored, so a newer build's file still loads here.
fn parse<T: Stored>(bytes: &[u8]) -> Option<T> {
    let text = std::str::from_utf8(bytes).ok()?;
    toml::from_str::<T>(text).ok().map(T::validated)
}

fn backup(path: &Path) -> PathBuf {
    sibling(path, ".bak")
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    path.with_file_name(name)
}

/// A temp file in the same folder, flushed, then renamed over the old one: a crash or a full
/// disk mid-write leaves the old file whole, never half a new one.
fn write_atomically(path: &Path, value: &impl Serialize) -> io::Result<()> {
    // Fails only for a path that is not Unicode, which TOML cannot hold.
    let text = toml::to_string(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let temp = sibling(path, ".tmp");
    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        // Replaces the target on Windows.
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info_overlay::domain::InfoOverlay;

    fn changed() -> Preferences {
        Preferences {
            advance_limit_ms: 250,
            folder_filter: vec!["private".to_owned(), "Old Stuff".to_owned()],
            info_overlay: InfoOverlay::Full,
            ..Preferences::default()
        }
    }

    #[test]
    fn saved_preferences_load_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Preferences::FILE_NAME);
        let (mut file, loaded, problem) = TomlFile::<Preferences>::load(path.clone());
        assert_eq!((loaded, problem), (Preferences::default(), None));
        assert!(file.save(&changed()).is_none());
        assert_eq!(TomlFile::<Preferences>::load(path.clone()).1, changed());
        // Saved again over the old file, with no temp file left behind.
        let again = Preferences {
            slideshow_interval_s: 3,
            ..changed()
        };
        assert!(file.save(&again).is_none());
        assert_eq!(TomlFile::<Preferences>::load(path).1, again);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn an_unchanged_save_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Preferences::FILE_NAME);
        let (mut file, loaded, _) = TomlFile::<Preferences>::load(path.clone());
        assert!(file.save(&loaded).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn a_file_missing_fields_loads_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Preferences::FILE_NAME);
        fs::write(
            &path,
            "info_overlay = \"file_name\"\nunknown_from_a_newer_build = 1\n",
        )
        .unwrap();
        let (_, loaded, problem) = TomlFile::<Preferences>::load(path);
        assert_eq!(problem, None);
        assert_eq!(
            loaded,
            Preferences {
                info_overlay: InfoOverlay::FileName,
                ..Preferences::default()
            }
        );
    }

    #[test]
    fn a_damaged_file_becomes_bak_and_defaults_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Preferences::FILE_NAME);
        let damaged = "advance_limit_ms = \"fast\"\n";
        fs::write(&path, damaged).unwrap();
        let (mut file, loaded, problem) = TomlFile::<Preferences>::load(path.clone());
        assert_eq!(
            (loaded, problem),
            (Preferences::default(), Some(LoadProblem::Damaged))
        );
        assert!(!path.exists());
        let bak = dir.path().join("superslide-prefs.toml.bak");
        assert_eq!(fs::read_to_string(&bak).unwrap(), damaged);
        // The next save writes a fresh file and leaves the backup alone.
        assert!(file.save(&changed()).is_none());
        assert_eq!(TomlFile::<Preferences>::load(path).1, changed());
        assert_eq!(fs::read_to_string(bak).unwrap(), damaged);
    }

    #[test]
    fn bytes_that_are_not_text_are_damage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Preferences::FILE_NAME);
        fs::write(&path, [0xff, 0xfe, 0x00, 0x41]).unwrap();
        assert_eq!(
            TomlFile::<Preferences>::load(path).2,
            Some(LoadProblem::Damaged)
        );
    }

    #[test]
    fn an_unread_file_is_replaced_by_the_first_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(Session::FILE_NAME);
        fs::write(&path, "an older run's session").unwrap();
        // Equal to the defaults, yet written: the file was never read.
        assert!(
            TomlFile::unread(path.clone())
                .save(&Session::default())
                .is_none()
        );
        let (_, loaded, problem) = TomlFile::<Session>::load(path);
        assert_eq!((loaded, problem), (Session::default(), None));
    }

    #[test]
    fn an_unwritable_folder_fails_once_then_quietly() {
        let dir = tempfile::tempdir().unwrap();
        // A folder that does not exist stands in for Program Files: both refuse the temp file.
        let path = dir.path().join("missing").join(Preferences::FILE_NAME);
        let (mut file, _, _) = TomlFile::<Preferences>::load(path);
        assert!(file.save(&changed()).is_some());
        let again = Preferences {
            slideshow_interval_s: 3,
            ..changed()
        };
        assert!(file.save(&again).is_none());
    }
}
