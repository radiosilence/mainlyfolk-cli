//! One function per CLI subcommand. Owned by the CLI work.
//!
//! Every command builds an [`Archive`], asks it one question, and prints an
//! [`Output`] envelope. Errors propagate to `main`, which prints them in the
//! same envelope with `success: false` and exits non-zero — so a caller can
//! parse stdout unconditionally.

use crate::archive::Archive;
use crate::client::Client;
use crate::error::Result;

mod pages;
mod records;
mod songs;
mod waterways;

pub use pages::{latest, page};
pub use records::{album, artist, labels, records};
pub use songs::{child, laws, search, song};
pub use waterways::waterways;

/// Build the archive accessor a command works through.
pub fn archive(no_cache: bool) -> Result<Archive> {
    Ok(Archive::new(if no_cache {
        Client::uncached()?
    } else {
        Client::new()?
    }))
}

/// Report on, or empty, the on-disk page cache.
pub fn cache(clear: bool) -> Result<()> {
    let client = Client::new()?;
    if clear {
        let removed = client.clear_cache()?;
        crate::models::Output::<()>::success_msg(format!("Cleared {removed} cached pages")).print();
    } else {
        crate::models::Output::success(serde_json::json!({
            "userAgent": crate::client::USER_AGENT,
            "maxConcurrent": crate::client::MAX_CONCURRENT,
            "freshForSecs": crate::client::FRESH_FOR.as_secs(),
        }))
        .print();
    }
    Ok(())
}
