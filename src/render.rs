//! wgpu device and surface, image texture upload, mip generation, the quad pipeline and egui
//! painting. All of it runs on the main thread.

use serde::{Deserialize, Serialize};

/// Sampling when zoomed past native size. Minification always stays mipmapped linear, so a shrunk
/// image never aliases whichever filter is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MagnificationFilter {
    #[default]
    Linear,
    /// Sharp pixels at high zoom.
    Nearest,
}
