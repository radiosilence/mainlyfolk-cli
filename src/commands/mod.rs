//! One function per CLI subcommand. Owned by the CLI work.
//!
//! Every command builds an [`Archive`], asks it one question, and prints an
//! [`Output`] envelope. Errors propagate to `main`, which prints them in the
//! same envelope with `success: false` and exits non-zero — so a caller can
//! parse stdout unconditionally.

use crate::archive::Archive;
use crate::client::Client;
use crate::error::Result;

/// Build the archive accessor a command works through.
pub fn archive(no_cache: bool) -> Result<Archive> {
    Ok(Archive::new(if no_cache {
        Client::uncached()?
    } else {
        Client::new()?
    }))
}

macro_rules! todo_command {
    ($($name:ident ( $($arg:ident : $ty:ty),* $(,)? )),* $(,)?) => {
        $(
            pub async fn $name(no_cache: bool, $($arg: $ty),*) -> Result<()> {
                let _ = (no_cache, $($arg),*);
                todo!(concat!(stringify!($name), " is not implemented yet"))
            }
        )*
    };
}

todo_command! {
    search(
        query: Option<&str>,
        author: Option<&str>,
        scheme: Option<crate::archive::RefKey>,
        number: Option<&str>,
    ),
    song(path: &str, lyrics_only: bool),
    child(filter: Option<&str>),
    laws(filter: Option<&str>),
    artist(name: &str),
    records(query: &str),
    album(path: &str),
    labels(),
    books(filter: Option<&str>),
    waterways(query: Option<&str>),
    page(path: &str),
    latest(),
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
