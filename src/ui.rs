//! egui content drawn over the image: info overlay, notices and the preferences screen.

#![expect(
    dead_code,
    reason = "scaffold: egui content arrives with the egui layer"
)]

/// What the info overlay shows; I cycles through these in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InfoOverlay {
    #[default]
    Hidden,
    FileName,
    Full,
}
