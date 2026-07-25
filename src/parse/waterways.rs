//! waterwaysongs.info parsers. Owned by the waterways-parser work.
use crate::error::{Error, Result};
use crate::models::WaterwaysSong;

pub fn song(_html: &str, path: &str) -> Result<WaterwaysSong> {
    Err(Error::Parse {
        what: "waterways song",
        url: path.into(),
    })
}
pub fn index(_html: &str) -> Vec<WaterwaysSong> {
    Vec::new()
}
