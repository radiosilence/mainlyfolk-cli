//! Artists, discographies, record search, albums, and labels.

use serde::Serialize;

use crate::archive::Archive;
use crate::error::{Error, Result};
use crate::models::{Album, Artist, Output, Track};

use super::archive;

/// An artist plus their chronological discography, as `folk artist` emits it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtistResult {
    artist: Artist,
    discography: Vec<Album>,
}

/// `name` may be a path (`/martin.carthy/`) or a display name ("Martin
/// Carthy"). A path is used as-is; a name has to be resolved, since the
/// archive has no artist-name search of its own — only a record search.
///
/// That resolution is lossy by the archive's own filing, not a bug here:
/// searching "Young Tradition" turns up releases the archive credits under
/// `/peter.bellamy/`, because that's genuinely where it filed them. A name
/// search can legitimately land on a different artist's page than the one
/// asked for.
pub async fn artist(no_cache: bool, name: &str) -> Result<()> {
    let archive = archive(no_cache)?;
    let artist = if name.starts_with('/') {
        archive.artist(name).await?
    } else {
        resolve_by_name(&archive, name).await?
    };
    let discography = match &artist.discography_path {
        Some(path) => archive.discography(path).await?,
        None => Vec::new(),
    };
    Output::success(ArtistResult {
        artist,
        discography,
    })
    .print();
    Ok(())
}

/// Record search groups results by credited artist but hands back no artist
/// path — only album paths. mainlynorfolk.info files every page for an artist
/// under one `/<slug>/`, album pages included, so the slug is recovered from
/// the best-matching group's albums rather than a second search.
async fn resolve_by_name(archive: &Archive, name: &str) -> Result<Artist> {
    let groups = archive.search_records(name).await?;
    let try_again = format!("Try `folk records {name}` to see what the archive has.");

    let (matched_name, albums) = groups
        .iter()
        .find(|(artist, _)| artist.eq_ignore_ascii_case(name))
        .or_else(|| groups.first())
        .ok_or_else(|| Error::NotFound(format!("No artist matching {name:?}. {try_again}")))?;

    let artist_path = resolve_slug(albums).ok_or_else(|| {
        Error::NotFound(format!(
            "No artist-filed release found for {matched_name:?} to resolve a page from — it may be a compilation or various-artists credit only. {try_again}"
        ))
    })?;

    archive.artist(&artist_path).await
}

/// The first album in the group that actually lives under an artist's own
/// `/<slug>/` — a various-artists release or compilation is filed under
/// `/folk/records/` instead, and a group can hold either kind alongside
/// artist-filed ones, so every album is tried in order rather than just the
/// first.
fn resolve_slug(albums: &[Album]) -> Option<String> {
    albums.iter().find_map(|a| artist_slug(&a.path))
}

/// The `/<slug>/` an album, song, or any other artist-filed page lives under —
/// `None` for paths under one of the site's structural sections, which look
/// like a slug but aren't a person.
fn artist_slug(path: &str) -> Option<String> {
    const NOT_AN_ARTIST: &[&str] = &["folk", "css", "images"];
    let first = path.strip_prefix('/')?.split('/').next()?;
    (!first.is_empty() && !NOT_AN_ARTIST.contains(&first)).then(|| format!("/{first}/"))
}

/// One credited-artist group from a record search, reshaped from the tuple
/// `Archive::search_records` returns into named fields for the JSON output.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordGroup {
    artist: String,
    albums: Vec<Album>,
}

pub async fn records(no_cache: bool, query: &str) -> Result<()> {
    let archive = archive(no_cache)?;
    let groups: Vec<RecordGroup> = archive
        .search_records(query)
        .await?
        .into_iter()
        .map(|(artist, albums)| RecordGroup { artist, albums })
        .collect();
    Output::success(groups).print();
    Ok(())
}

pub async fn album(no_cache: bool, path: &str) -> Result<()> {
    let archive = archive(no_cache)?;
    let (album, tracks) = archive.album(path).await?;
    Output::success(AlbumResult { album, tracks }).print();
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AlbumResult {
    album: Album,
    tracks: Vec<Track>,
}

pub async fn labels(no_cache: bool) -> Result<()> {
    let archive = archive(no_cache)?;
    Output::success(archive.labels().await?).print();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Source;

    fn album(path: &str) -> Album {
        Album {
            path: path.into(),
            title: "x".into(),
            artist: None,
            year: None,
            label: None,
            catalogue_number: None,
            format: None,
            cover_url: None,
            source: Source::MainlyNorfolk,
        }
    }

    #[test]
    fn artist_slug_is_the_first_path_segment() {
        assert_eq!(
            artist_slug("/martin.carthy/records/whatnews.html"),
            Some("/martin.carthy/".to_string())
        );
    }

    #[test]
    fn a_path_with_no_segment_has_no_slug() {
        assert_eq!(artist_slug("/"), None);
    }

    #[test]
    fn a_compilation_filed_under_folk_records_has_no_artist_slug() {
        assert_eq!(
            artist_slug("/folk/records/fromthehumbertothetweed.html"),
            None
        );
    }

    #[test]
    fn a_group_resolves_via_a_later_artist_filed_album_when_the_first_is_a_compilation() {
        let albums = [
            album("/folk/records/fromthehumbertothetweed.html"),
            album("/watersons/records/frostandfire.html"),
        ];
        assert_eq!(resolve_slug(&albums), Some("/watersons/".to_string()));
    }

    #[test]
    fn a_group_of_only_compilations_has_no_slug_to_resolve() {
        let albums = [album("/folk/records/fromthehumbertothetweed.html")];
        assert_eq!(resolve_slug(&albums), None);
    }
}
