use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerMode {
    New,
    Memorable,
    PassDefault,
}

pub(super) const CREATE_CONTENT_HEIGHT: f32 = 100.;
pub(super) const CREATE_OPTIONS_HEIGHT: f32 = 80.;
pub(super) const CREATE_CONTENT_LABEL_HEIGHT: f32 = 16.;
pub(super) const CREATE_PANEL_MIN_HEIGHT: f32 = 560.;

pub(super) struct OverwriteConfirmation {
    pub(super) entry: Entry,
}
