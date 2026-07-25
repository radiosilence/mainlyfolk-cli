//! Artist, discography and record-search parsers. Owned by the artist-parser work.
use crate::error::{Error, Result};
use crate::models::{Album, Artist};

pub fn parse(_html: &str, path: &str) -> Result<Artist> {
    Err(Error::Parse {
        what: "artist",
        url: path.into(),
    })
}
pub fn discography(_html: &str, _base: &str) -> Vec<Album> {
    Vec::new()
}
pub fn records_search(_html: &str, _base: &str) -> Vec<(String, Vec<Album>)> {
    Vec::new()
}
