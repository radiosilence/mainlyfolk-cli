//! Album page parser. Owned by the artist-parser work.
use crate::error::{Error, Result};
use crate::models::{Album, Track};

pub fn parse(_html: &str, path: &str) -> Result<(Album, Vec<Track>)> {
    Err(Error::Parse {
        what: "album",
        url: path.into(),
    })
}
