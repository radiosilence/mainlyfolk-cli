//! Typed accessors over the archives: fetch a page, parse it, hand back a model.
//!
//! The one layer that pairs [`crate::client`] with [`crate::parse`]. Both the
//! CLI and the GraphQL resolvers go through here, so there is one answer to
//! "what is a song page" rather than two that drift.
//!
//! # Searching beats scraping
//!
//! mainlynorfolk.info publishes two `search.php` endpoints — one for songs, one
//! for records — and they are the right way to look things up. The full song
//! index is 670KB of HTML listing 5,391 songs; the same query through
//! `search.php` is a 10KB response the server has already filtered. Anything
//! answerable by search goes through [`Archive::search_songs`] or
//! [`Archive::search_records`]; the whole-index parsers exist for browsing the
//! Child and Laws canons, where the entire list *is* the answer.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::models::{
    Album, Artist, Book, Label, Page, Song, SongSummary, Source, Track, WaterwaysSong,
};
use crate::parse;

/// Paths that are structural rather than content — the entry points every
/// other lookup starts from.
pub mod paths {
    pub const FOLK: &str = "/folk/";
    pub const SONG_INDEX: &str = "/folk/songs/";
    pub const CHILD_INDEX: &str = "/folk/songs/childindex.html";
    pub const LAWS_INDEX: &str = "/folk/songs/lawsindex.html";
    pub const SONG_SEARCH: &str = "/folk/songs/search.php";
    pub const RECORDS_INDEX: &str = "/folk/records/";
    pub const RECORD_SEARCH: &str = "/folk/records/search.php";
    pub const LATEST_CHANGES: &str = "/folk/latestchanges.html";
    pub const BOOKS: &str = "/folk/books/";
    pub const WATERWAYS_MENU: &str = "/songmenu.htm";
}

/// Which reference scheme a numeric song search is against. Mirrors the
/// `key` select on the archive's own search form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKey {
    Roud,
    Child,
    Laws,
    GreigDuncan,
    Henry,
}

impl RefKey {
    /// The value the archive's form expects.
    pub fn as_form_value(self) -> &'static str {
        match self {
            RefKey::Roud => "roud",
            RefKey::Child => "child",
            RefKey::Laws => "laws",
            RefKey::GreigDuncan => "gd",
            RefKey::Henry => "henry",
        }
    }
}

/// A song search, as the archive's `search.php` accepts it.
///
/// Fields combine: a title *and* an author narrows. All-empty is rejected
/// rather than sent, since the endpoint answers that with the entire index.
#[derive(Debug, Clone, Default)]
pub struct SongQuery {
    /// Title or part of one. A trailing `*` is a wildcard, server-side.
    pub title: Option<String>,
    /// Author/performer name or part of one.
    pub author: Option<String>,
    /// Reference scheme to match `keyval` against.
    pub key: Option<RefKey>,
    /// The reference number, e.g. `"12"` for Roud 12, `"P15"` for Laws P15.
    pub keyval: Option<String>,
}

impl SongQuery {
    pub fn is_empty(&self) -> bool {
        self.title.as_deref().unwrap_or("").trim().is_empty()
            && self.author.as_deref().unwrap_or("").trim().is_empty()
            && self.keyval.as_deref().unwrap_or("").trim().is_empty()
    }

    /// Form fields for `search.php`, omitting anything unset.
    pub fn to_form(&self) -> Vec<(&'static str, String)> {
        let mut form = Vec::new();
        if let Some(title) = &self.title {
            form.push(("song", title.clone()));
        }
        if let Some(author) = &self.author {
            form.push(("author", author.clone()));
        }
        if let Some(keyval) = &self.keyval {
            // The endpoint reads `keyval` only alongside a `key`; Roud is the
            // archive's own default and the broadest scheme.
            form.push((
                "key",
                self.key.unwrap_or(RefKey::Roud).as_form_value().into(),
            ));
            form.push(("keyval", keyval.clone()));
        }
        form
    }
}

/// The archives, as typed lookups.
#[derive(Clone)]
pub struct Archive {
    client: Client,
}

impl Archive {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    // ============ Songs ============

    /// One song page in full.
    pub async fn song(&self, path: &str) -> Result<Song> {
        let html = self.client.get(Source::MainlyNorfolk, path).await?;
        parse::song(&html, path)
    }

    /// Search songs through the archive's own endpoint.
    pub async fn search_songs(&self, query: &SongQuery) -> Result<Vec<SongSummary>> {
        if query.is_empty() {
            return Err(Error::InvalidArgument(
                "A song search needs a title, an author, or a reference number.".into(),
            ));
        }
        let owned = query.to_form();
        let form: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let html = self
            .client
            .post_form(Source::MainlyNorfolk, paths::SONG_SEARCH, &form)
            .await?;
        Ok(parse::song_list(&html, paths::SONG_SEARCH))
    }

    /// Every Child ballad the archive covers.
    pub async fn child_index(&self) -> Result<Vec<SongSummary>> {
        let html = self
            .client
            .get(Source::MainlyNorfolk, paths::CHILD_INDEX)
            .await?;
        Ok(parse::song_list(&html, paths::CHILD_INDEX))
    }

    /// Every Laws ballad the archive covers.
    pub async fn laws_index(&self) -> Result<Vec<SongSummary>> {
        let html = self
            .client
            .get(Source::MainlyNorfolk, paths::LAWS_INDEX)
            .await?;
        Ok(parse::song_list(&html, paths::LAWS_INDEX))
    }

    /// The full song index — every song and tune on the archive.
    ///
    /// 670KB and several thousand entries. Prefer [`Self::search_songs`] for
    /// anything a query can answer.
    pub async fn song_index(&self) -> Result<Vec<SongSummary>> {
        let html = self
            .client
            .get(Source::MainlyNorfolk, paths::SONG_INDEX)
            .await?;
        Ok(parse::song_list(&html, paths::SONG_INDEX))
    }

    // ============ Artists, records, labels ============

    /// An artist's index page.
    pub async fn artist(&self, path: &str) -> Result<Artist> {
        let html = self.client.get(Source::MainlyNorfolk, path).await?;
        parse::artist(&html, path)
    }

    /// An artist's chronological discography.
    pub async fn discography(&self, path: &str) -> Result<Vec<Album>> {
        let html = self.client.get(Source::MainlyNorfolk, path).await?;
        Ok(parse::discography(&html, path))
    }

    /// Search releases by artist or album name, through the archive's endpoint.
    /// Grouped by credited artist, as the archive groups them.
    pub async fn search_records(&self, query: &str) -> Result<Vec<(String, Vec<Album>)>> {
        if query.trim().is_empty() {
            return Err(Error::InvalidArgument(
                "A record search needs an artist or album name.".into(),
            ));
        }
        let html = self
            .client
            .post_form(
                Source::MainlyNorfolk,
                paths::RECORD_SEARCH,
                &[("artist", query)],
            )
            .await?;
        Ok(parse::records_search(&html, paths::RECORD_SEARCH))
    }

    /// One release and its tracks. `path` may carry a `#fragment` selecting one
    /// release from a page documenting several.
    pub async fn album(&self, path: &str) -> Result<(Album, Vec<Track>)> {
        let html = self.client.get(Source::MainlyNorfolk, path).await?;
        parse::album(&html, path)
    }

    /// Every label discography the archive publishes, read from its navigation.
    pub async fn labels(&self) -> Result<Vec<Label>> {
        let html = self.client.get(Source::MainlyNorfolk, paths::FOLK).await?;
        Ok(parse::labels(&html))
    }

    /// A label's discography.
    pub async fn label_albums(&self, path: &str) -> Result<Vec<Album>> {
        let html = self.client.get(Source::MainlyNorfolk, path).await?;
        Ok(parse::discography(&html, path))
    }

    // ============ Bibliography ============

    /// Every book in the archive's bibliography.
    ///
    /// One page, so this is a single fetch — which is why song-page citations
    /// are resolved by looking them up here rather than by following each
    /// citation's own link.
    pub async fn books(&self) -> Result<Vec<Book>> {
        let html = self.client.get(Source::MainlyNorfolk, paths::BOOKS).await?;
        Ok(parse::books(&html, paths::BOOKS))
    }

    // ============ Waterways ============

    /// Every canal song in the waterwaysongs.info menu. Titles and paths only.
    pub async fn waterways_index(&self) -> Result<Vec<WaterwaysSong>> {
        let html = self
            .client
            .get(Source::Waterways, paths::WATERWAYS_MENU)
            .await?;
        Ok(parse::waterways_index(&html))
    }

    /// One canal song page in full.
    pub async fn waterways_song(&self, path: &str) -> Result<WaterwaysSong> {
        let html = self.client.get(Source::Waterways, path).await?;
        parse::waterways_song(&html, path)
    }

    // ============ Anything else ============

    /// Any page on either archive, as text and links.
    pub async fn page(&self, source: Source, path: &str) -> Result<Page> {
        let html = self.client.get(source, path).await?;
        parse::page(&html, path, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_is_rejected_before_it_reaches_the_archive() {
        assert!(SongQuery::default().is_empty());
        assert!(
            SongQuery {
                title: Some("   ".into()),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !SongQuery {
                title: Some("reynardine".into()),
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn a_reference_search_always_sends_the_scheme_with_the_number() {
        // `keyval` without `key` is ignored by the endpoint, which silently
        // returns the whole index — so the pair is never split.
        let form = SongQuery {
            keyval: Some("2".into()),
            key: Some(RefKey::Child),
            ..Default::default()
        }
        .to_form();
        assert!(form.contains(&("key", "child".to_string())));
        assert!(form.contains(&("keyval", "2".to_string())));
    }

    #[test]
    fn a_bare_number_defaults_to_roud() {
        let form = SongQuery {
            keyval: Some("12".into()),
            ..Default::default()
        }
        .to_form();
        assert!(form.contains(&("key", "roud".to_string())));
    }

    #[test]
    fn unset_fields_are_omitted_rather_than_sent_blank() {
        let form = SongQuery {
            title: Some("reynardine".into()),
            ..Default::default()
        }
        .to_form();
        assert_eq!(form, [("song", "reynardine".to_string())]);
    }
}
