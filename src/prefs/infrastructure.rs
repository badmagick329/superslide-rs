//! `superslide-prefs.toml`, beside the executable so a copied folder carries it.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::prefs::domain::Preferences;

pub const FILE_NAME: &str = "superslide-prefs.toml";

/// Why the app runs on defaults. `Display` is the notice's reason.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadProblem {
    /// Not a preferences file this build can read. It was renamed to `.bak`, so the next save
    /// does not destroy what the owner wrote.
    Damaged,
    /// Damaged, and the rename failed too; the next save replaces it.
    DamagedNotKept(String),
    Unreadable(String),
}

/// The file, and what it last held, so a save only writes a change.
#[derive(Debug)]
pub struct PrefsFile {
    path: PathBuf,
    saved: Preferences,
    /// A save has failed this run: the owner has heard, once.
    failed: bool,
}

impl PrefsFile {
    /// Beside the executable. A missing file is not a problem: the defaults are what it would
    /// hold.
    pub fn beside_exe() -> (Option<Self>, Preferences, Option<LoadProblem>) {
        match std::env::current_exe() {
            Ok(exe) => {
                let (file, preferences, problem) = Self::load(exe.with_file_name(FILE_NAME));
                (Some(file), preferences, problem)
            }
            Err(e) => (
                None,
                Preferences::default(),
                Some(LoadProblem::Unreadable(e.to_string())),
            ),
        }
    }

    pub fn load(path: PathBuf) -> (Self, Preferences, Option<LoadProblem>) {
        let (preferences, problem) = match fs::read(&path) {
            Ok(bytes) => match parse(&bytes) {
                Some(preferences) => (preferences, None),
                None => {
                    let problem = match fs::rename(&path, backup(&path)) {
                        Ok(()) => LoadProblem::Damaged,
                        Err(e) => LoadProblem::DamagedNotKept(e.to_string()),
                    };
                    (Preferences::default(), Some(problem))
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => (Preferences::default(), None),
            Err(e) => (
                Preferences::default(),
                Some(LoadProblem::Unreadable(e.to_string())),
            ),
        };
        let file = Self {
            path,
            saved: preferences.clone(),
            failed: false,
        };
        (file, preferences, problem)
    }

    /// Writes `preferences` if they changed since the last write. Returns the error only for the
    /// run's first failure, so an unwritable folder (Program Files) gives one notice, not one per
    /// change; later saves still try, in case the folder became writable.
    pub fn save(&mut self, preferences: &Preferences) -> Option<io::Error> {
        if *preferences == self.saved {
            return None;
        }
        match write_atomically(&self.path, preferences) {
            Ok(()) => {
                self.saved = preferences.clone();
                None
            }
            Err(e) => (!std::mem::replace(&mut self.failed, true)).then_some(e),
        }
    }
}

/// `None` for anything but a TOML table of known types: invalid UTF-8, bad syntax, or a field of
/// the wrong type. Unknown fields are ignored, so a newer build's file still loads here.
fn parse(bytes: &[u8]) -> Option<Preferences> {
    let text = std::str::from_utf8(bytes).ok()?;
    toml::from_str::<Preferences>(text)
        .ok()
        .map(Preferences::clamped)
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
fn write_atomically(path: &Path, preferences: &Preferences) -> io::Result<()> {
    let text = toml::to_string(preferences).expect("Preferences always serialise");
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
        let path = dir.path().join(FILE_NAME);
        let (mut file, loaded, problem) = PrefsFile::load(path.clone());
        assert_eq!((loaded, problem), (Preferences::default(), None));
        assert!(file.save(&changed()).is_none());
        assert_eq!(PrefsFile::load(path.clone()).1, changed());
        // Saved again over the old file, with no temp file left behind.
        let again = Preferences {
            slideshow_interval_s: 3,
            ..changed()
        };
        assert!(file.save(&again).is_none());
        assert_eq!(PrefsFile::load(path).1, again);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn an_unchanged_save_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let (mut file, loaded, _) = PrefsFile::load(path.clone());
        assert!(file.save(&loaded).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn a_file_missing_fields_loads_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(
            &path,
            "info_overlay = \"file_name\"\nunknown_from_a_newer_build = 1\n",
        )
        .unwrap();
        let (_, loaded, problem) = PrefsFile::load(path);
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
        let path = dir.path().join(FILE_NAME);
        let damaged = "advance_limit_ms = \"fast\"\n";
        fs::write(&path, damaged).unwrap();
        let (mut file, loaded, problem) = PrefsFile::load(path.clone());
        assert_eq!(
            (loaded, problem),
            (Preferences::default(), Some(LoadProblem::Damaged))
        );
        assert!(!path.exists());
        let bak = dir.path().join("superslide-prefs.toml.bak");
        assert_eq!(fs::read_to_string(&bak).unwrap(), damaged);
        // The next save writes a fresh file and leaves the backup alone.
        assert!(file.save(&changed()).is_none());
        assert_eq!(PrefsFile::load(path).1, changed());
        assert_eq!(fs::read_to_string(bak).unwrap(), damaged);
    }

    #[test]
    fn bytes_that_are_not_text_are_damage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, [0xff, 0xfe, 0x00, 0x41]).unwrap();
        assert_eq!(PrefsFile::load(path).2, Some(LoadProblem::Damaged));
    }

    #[test]
    fn an_unwritable_folder_fails_once_then_quietly() {
        let dir = tempfile::tempdir().unwrap();
        // A folder that does not exist stands in for Program Files: both refuse the temp file.
        let path = dir.path().join("missing").join(FILE_NAME);
        let (mut file, _, _) = PrefsFile::load(path);
        assert!(file.save(&changed()).is_some());
        let again = Preferences {
            slideshow_interval_s: 3,
            ..changed()
        };
        assert!(file.save(&again).is_none());
    }
}
