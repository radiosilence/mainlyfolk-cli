//! Song-list and label parsers. Owned by the song-parser work.
use crate::models::{Label, SongSummary};

pub fn songs(_html: &str, _base: &str) -> Vec<SongSummary> {
    Vec::new()
}
pub fn labels(_html: &str) -> Vec<Label> {
    Vec::new()
}
