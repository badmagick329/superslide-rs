//! Delete, move and copy on one thread of their own, in the order asked. The only writes the
//! viewer makes to image files, and none of them overwrites one.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader};
use std::iter;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

use crate::file_actions::domain::{Action, Failure, Transfer};

/// Read size for copies: a 50 MP JPEG is about 20 MB, so this is a few dozen reads.
const COPY_BUFFER: usize = 1 << 20;

/// The file action thread. A Recycle Bin call or a copy to a slow drive can take seconds, which
/// the event loop must not wait for. Dropping it waits for the actions already asked for: exiting
/// mid-copy would leave half a file behind.
pub struct FileActions {
    jobs: Option<mpsc::Sender<(PathBuf, Action)>>,
    thread: Option<JoinHandle<()>>,
}

impl FileActions {
    /// Each result is handed to `done` from the file action thread.
    pub fn spawn<F>(done: F) -> Self
    where
        F: Fn(PathBuf, Action, Result<(), Failure>) + Send + 'static,
    {
        let (jobs, queue) = mpsc::channel::<(PathBuf, Action)>();
        let thread = thread::Builder::new()
            .name("file actions".to_owned())
            .spawn(move || {
                for (path, action) in queue {
                    let result = run(&path, &action);
                    done(path, action, result);
                }
            })
            .expect("the OS starts a thread");
        Self {
            jobs: Some(jobs),
            thread: Some(thread),
        }
    }

    pub fn request(&self, path: PathBuf, action: Action) {
        let jobs = self.jobs.as_ref().expect("only `drop` takes the sender");
        // Fails only if the thread panicked, which `run` gives it no way to do.
        let _ = jobs.send((path, action));
    }
}

impl Drop for FileActions {
    fn drop(&mut self) {
        // Closing the channel ends the thread's loop once the queue is empty.
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(path: &Path, action: &Action) -> Result<(), Failure> {
    match action {
        // `trash` asks Windows to warn first where a drive has no Recycle Bin (network shares),
        // so nothing is deleted for good without a prompt.
        Action::Delete => trash::delete(path).map_err(|e| Failure::Failed(e.to_string())),
        Action::Transfer(transfer, folder) => {
            let Some(name) = path.file_name() else {
                return Err(Failure::Failed("not a file".to_owned()));
            };
            let target = folder.join(name);
            // Also catches a move or copy into the image's own folder, which `MoveFileExW`
            // would report as done. The no-overwrite calls below still guard the race.
            if fs::symlink_metadata(&target).is_ok() {
                return Err(Failure::Exists);
            }
            match transfer {
                Transfer::Copy => copy_new(path, &target).map_err(failed),
                Transfer::Move => match rename_new(path, &target) {
                    Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
                        move_across(path, &target)
                    }
                    result => result.map_err(failed),
                },
            }
        }
    }
}

fn failed(e: io::Error) -> Failure {
    match e.kind() {
        io::ErrorKind::AlreadyExists => Failure::Exists,
        _ => Failure::Failed(e.to_string()),
    }
}

/// A rename that fails if `target` exists. `std::fs::rename` replaces it on Windows. Fails with
/// `CrossesDevices` between drives, where a rename is not possible.
fn rename_new(source: &Path, target: &Path) -> io::Result<()> {
    let wide = |path: &Path| -> Vec<u16> {
        OsStr::encode_wide(path.as_os_str())
            .chain(iter::once(0))
            .collect()
    };
    let (source, target) = (wide(source), wide(target));
    // SAFETY: both are NUL-terminated UTF-16 strings that outlive the call. Flags 0: neither
    // replace an existing file nor fall back to a copy.
    match unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) } {
        0 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

/// Copy, then delete the original only once the copy is complete and on disk. A copy that fails,
/// such as on a full drive, leaves the original as it was and no partial file.
fn move_across(source: &Path, target: &Path) -> Result<(), Failure> {
    copy_new(source, target).map_err(failed)?;
    fs::remove_file(source).map_err(|e| Failure::OriginalKept(e.to_string()))
}

/// Copies to a `target` that must not exist yet, keeping the modified time so date sort places
/// the copy where it placed the original.
fn copy_new(source: &Path, target: &Path) -> io::Result<()> {
    let from = File::open(source)?;
    let modified = from.metadata()?.modified()?;
    let mut from = BufReader::with_capacity(COPY_BUFFER, from);
    write_new(target, |to| {
        io::copy(&mut from, to)?;
        to.set_modified(modified)?;
        to.sync_all()
    })
}

/// Creates `target`, failing if it exists, and runs `fill` on it. If `fill` fails, the partial
/// file goes; a file that was there first is never touched, since creating it failed.
fn write_new(target: &Path, fill: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let filled = fill(&mut file);
    drop(file);
    if filled.is_err() {
        let _ = fs::remove_file(target);
    }
    filled
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::{Duration, SystemTime};

    use super::*;

    /// A folder holding `a/photo.jpg` and an empty `b/`.
    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let source = a.join("photo.jpg");
        fs::write(&source, b"original").unwrap();
        (dir, source, b)
    }

    fn transfer(transfer: Transfer, source: &Path, folder: &Path) -> Result<(), Failure> {
        run(source, &Action::Transfer(transfer, folder.to_path_buf()))
    }

    #[test]
    fn a_name_collision_leaves_both_files_untouched() {
        for kind in [Transfer::Move, Transfer::Copy] {
            let (_dir, source, b) = setup();
            fs::write(b.join("photo.jpg"), b"already there").unwrap();
            assert_eq!(transfer(kind, &source, &b), Err(Failure::Exists));
            assert_eq!(fs::read(&source).unwrap(), b"original");
            assert_eq!(fs::read(b.join("photo.jpg")).unwrap(), b"already there");
        }
    }

    #[test]
    fn a_transfer_into_its_own_folder_is_a_collision() {
        for kind in [Transfer::Move, Transfer::Copy] {
            let (_dir, source, _) = setup();
            let folder = source.parent().unwrap();
            assert_eq!(transfer(kind, &source, folder), Err(Failure::Exists));
            assert_eq!(fs::read(&source).unwrap(), b"original");
        }
    }

    #[test]
    fn rename_new_refuses_an_existing_target() {
        let (_dir, source, b) = setup();
        let target = b.join("photo.jpg");
        fs::write(&target, b"already there").unwrap();
        let error = rename_new(&source, &target).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"already there");
    }

    #[test]
    fn move_and_copy_carry_the_bytes_and_modified_time() {
        let (_dir, source, b) = setup();
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
        File::options()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        transfer(Transfer::Copy, &source, &b).unwrap();
        let copy = b.join("photo.jpg");
        assert_eq!(fs::read(&copy).unwrap(), b"original");
        assert_eq!(fs::metadata(&copy).unwrap().modified().unwrap(), modified);
        assert!(source.exists());

        fs::remove_file(&copy).unwrap();
        transfer(Transfer::Move, &source, &b).unwrap();
        assert_eq!(fs::read(&copy).unwrap(), b"original");
        assert!(!source.exists());
    }

    /// The path a move between drives takes, run within one.
    #[test]
    fn a_move_across_drives_deletes_the_original_only_after_copying() {
        let (_dir, source, b) = setup();
        let target = b.join("photo.jpg");
        move_across(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert!(!source.exists());

        // A source that cannot be read stops the move before anything is written.
        let missing = source.with_file_name("gone.jpg");
        let result = move_across(&missing, &b.join("gone.jpg"));
        assert!(matches!(result, Err(Failure::Failed(_))));
        assert!(!b.join("gone.jpg").exists());
    }

    #[test]
    fn a_failed_write_leaves_no_partial_file() {
        let (_dir, _, b) = setup();
        let target = b.join("partial.jpg");
        let result = write_new(&target, |file| {
            file.write_all(b"half")?;
            Err(io::Error::other("the drive is full"))
        });
        assert!(result.is_err());
        assert!(!target.exists());

        // A file already there makes creation fail, so it is left alone.
        fs::write(&target, b"already there").unwrap();
        assert!(write_new(&target, |_| Ok(())).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"already there");
    }
}
