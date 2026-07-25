//! Any archive page as plain text, and the "latest changes" shortcut.

use crate::archive::paths;
use crate::error::Result;
use crate::models::{Output, Source};

use super::archive;

pub async fn page(no_cache: bool, path: &str) -> Result<()> {
    let archive = archive(no_cache)?;
    Output::success(archive.page(source_of(path), path).await?).print();
    Ok(())
}

pub async fn latest(no_cache: bool) -> Result<()> {
    let archive = archive(no_cache)?;
    Output::success(
        archive
            .page(Source::MainlyNorfolk, paths::LATEST_CHANGES)
            .await?,
    )
    .print();
    Ok(())
}

/// Which archive a path belongs to. [`Source::of_url`] already recognises a
/// full URL on either site; the one case it can't decide is a bare
/// waterwaysongs.info path, which always starts `/Songs/`.
fn source_of(path: &str) -> Source {
    Source::of_url(path).unwrap_or_else(|| {
        if path.starts_with("/Songs/") {
            Source::Waterways
        } else {
            Source::MainlyNorfolk
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waterways_url_selects_waterways() {
        assert_eq!(
            source_of("https://www.waterwaysongs.info/Songs/H/hard.htm"),
            Source::Waterways
        );
    }

    #[test]
    fn a_waterways_path_selects_waterways() {
        assert_eq!(source_of("/Songs/H/hard.htm"), Source::Waterways);
    }

    #[test]
    fn a_mainlynorfolk_path_selects_mainlynorfolk() {
        assert_eq!(
            source_of("/martin.carthy/songs/theelfinknight.html"),
            Source::MainlyNorfolk
        );
    }

    #[test]
    fn a_mainlynorfolk_url_selects_mainlynorfolk() {
        assert_eq!(
            source_of("https://www.mainlynorfolk.info/folk/"),
            Source::MainlyNorfolk
        );
    }
}
