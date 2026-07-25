//! Song page parser. Owned by the song-parser work; see `parse::song`.
use crate::error::{Error, Result};
use crate::models::Song;

pub fn parse(_html: &str, path: &str) -> Result<Song> {
    Err(Error::Parse {
        what: "song",
        url: path.into(),
    })
}
