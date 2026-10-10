//! Ctrl+C and Ctrl+V on a thread of their own, through `arboard`. A copy re-decodes the file at
//! full size and arboard encodes it as PNG as well as a bitmap, which takes seconds for a large
//! photo; the event loop must not wait for that.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use arboard::{Clipboard, ImageData};

use crate::clipboard::domain::{CopyFailure, PasteFailure, check_pasted};
use crate::image_file::domain::Orientation;
use crate::load::domain::{DecodeError, Decoded};
use crate::view::domain::upright_pixels;

/// Pixels of a size shrunk to fit a maximum side, as `load` does for the GPU.
type ShrinkToFit = fn([u32; 2], Vec<u8>, u32) -> ([u32; 2], Vec<u8>);

/// What the clipboard needs from `load`'s decoders, handed in by `shell`: a slice reaches
/// another slice's infrastructure only through the outer layer.
pub struct Loading {
    pub decode: fn(&Path, u32) -> Result<Decoded, DecodeError>,
    pub shrink_to_fit: ShrinkToFit,
    /// A pasted image is shrunk to this, as a decoded file is.
    pub max_texture_dimension: u32,
}

pub enum Request {
    /// `turns` is the image's screen rotation, so the copy matches the screen.
    Copy {
        path: PathBuf,
        turns: u8,
    },
    Paste,
}

pub enum Done {
    Copied {
        path: PathBuf,
        result: Result<(), CopyFailure>,
    },
    Pasted(Result<Decoded, PasteFailure>),
}

/// Requests run in the order made, so a paste after a copy reads that copy. Dropping it waits
/// for a copy in progress, so closing the window right after Ctrl+C still fills the clipboard.
pub struct ClipboardThread {
    requests: Option<mpsc::Sender<Request>>,
    thread: Option<JoinHandle<()>>,
}

impl ClipboardThread {
    pub fn spawn<F>(loading: Loading, done: F) -> Self
    where
        F: Fn(Done) + Send + 'static,
    {
        let (requests, queue) = mpsc::channel::<Request>();
        let thread = thread::Builder::new()
            .name("clipboard".to_owned())
            .spawn(move || {
                for request in queue {
                    done(match request {
                        Request::Copy { path, turns } => {
                            let result = copy(&loading, &path, turns);
                            Done::Copied { path, result }
                        }
                        Request::Paste => Done::Pasted(paste(&loading)),
                    });
                }
            })
            .expect("the OS starts a thread");
        Self {
            requests: Some(requests),
            thread: Some(thread),
        }
    }

    pub fn request(&self, request: Request) {
        let requests = self
            .requests
            .as_ref()
            .expect("only `drop` takes the sender");
        // Fails only if the thread panicked; the user then gets no notice for this request.
        let _ = requests.send(request);
    }
}

impl Drop for ClipboardThread {
    fn drop(&mut self) {
        self.requests = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Full size: the copy is for pasting elsewhere, not for this GPU, so no texture limit applies.
fn copy(loading: &Loading, path: &Path, turns: u8) -> Result<(), CopyFailure> {
    let decoded = (loading.decode)(path, u32::MAX)?;
    let (size, pixels) = upright_pixels(decoded.size, &decoded.pixels, decoded.orientation, turns);
    drop(decoded);
    let image = ImageData {
        width: size[0] as usize,
        height: size[1] as usize,
        bytes: pixels.into(),
    };
    Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_image(image))
        .map_err(|e| CopyFailure::Clipboard(e.to_string()))
}

fn paste(loading: &Loading) -> Result<Decoded, PasteFailure> {
    let image = Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_image())
        .map_err(|e| match e {
            arboard::Error::ContentNotAvailable => PasteFailure::NoImage,
            arboard::Error::ClipboardOccupied => PasteFailure::Busy,
            e => PasteFailure::Failed(e.to_string()),
        })?;
    let original_size = check_pasted(image.width, image.height, image.bytes.len())?;
    let (size, pixels) = (loading.shrink_to_fit)(
        original_size,
        image.bytes.into_owned(),
        loading.max_texture_dimension,
    );
    Ok(Decoded {
        size,
        original_size,
        pixels,
        orientation: Orientation::Normal,
    })
}
