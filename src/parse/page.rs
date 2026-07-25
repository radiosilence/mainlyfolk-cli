//! Generic page parser. Owned by the artist-parser work.
use crate::error::{Error, Result};
use crate::models::{Page, Source};

pub fn parse(_html: &str, path: &str, _source: Source) -> Result<Page> {
    Err(Error::Parse {
        what: "page",
        url: path.into(),
    })
}
