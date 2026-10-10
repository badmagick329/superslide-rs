/// What the info overlay shows; I cycles through these in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    dead_code,
    reason = "scaffold: the info overlay arrives with its issue"
)]
pub enum InfoOverlay {
    #[default]
    Hidden,
    FileName,
    Full,
}
