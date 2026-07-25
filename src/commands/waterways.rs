//! Canal and inland-waterways songs, from waterwaysongs.info.

use crate::error::Result;
use crate::models::Output;

use super::archive;

/// No query lists the whole menu. A query narrows by title, and when exactly
/// one song matches, that's almost certainly what was wanted — so the full
/// page is fetched and returned, rather than making the caller paste the path
/// back into a second command. Zero or multiple matches stay a list to search
/// within.
pub async fn waterways(no_cache: bool, query: Option<&str>) -> Result<()> {
    let archive = archive(no_cache)?;
    let index = archive.waterways_index().await?;

    let Some(query) = query else {
        Output::success(index).print();
        return Ok(());
    };

    let needle = query.to_ascii_lowercase();
    let matches: Vec<_> = index
        .into_iter()
        .filter(|s| s.title.to_ascii_lowercase().contains(&needle))
        .collect();

    if let [one] = matches.as_slice() {
        Output::success(archive.waterways_song(&one.path).await?).print();
    } else {
        Output::success(matches).print();
    }
    Ok(())
}
