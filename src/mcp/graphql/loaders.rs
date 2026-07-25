//! DataLoaders backing the nested GraphQL resolvers.
//!
//! Every read that leaves the process goes through one of these; no resolver
//! fetches a page itself. async-graphql resolves sibling fields and list
//! elements concurrently, so N resolvers each asking for one key collapse into a
//! single batched `load` per loader.
//!
//! **There is no bulk endpoint here, at all.** The archives are static websites
//! — one HTML document per song, per release, per artist — so "batched" never
//! means one request for many things. It means two cheaper things, and the
//! module is honest about which:
//!
//! - **Deduplication.** The graph has cycles by design (song → recordings →
//!   album → tracks → song), so a query walking back to a page it has already
//!   visited is the normal case rather than the exotic one. Inside one request
//!   that page is fetched once.
//! - **Concurrency, bounded.** Distinct pages go out together rather than one
//!   after another, through [`crate::client::MAX_CONCURRENT`] — four at a time,
//!   across both archives. That cap is the politeness budget, and it is the real
//!   ceiling on how hard any single query can hit a volunteer-run site.
//!
//! | Loader | Request | What batching buys |
//! | --- | --- | --- |
//! | [`PageLoader`] | one GET per distinct page | everything below goes through it; the only place a fetch happens |
//! | [`SongLoader`], [`AlbumLoader`], [`ArtistLoader`], [`DiscographyLoader`], [`WaterwaysLoader`] | their page, through [`PageLoader`] | dedup of the fetch *and* of the parse — two fields wanting the same album parse it once |
//! | [`IndexLoader`] | one GET per named index | the full song index is 670KB and several thousand entries; parsed once per request however many resolvers ask |
//! | [`SearchLoader`], [`RecordsSearchLoader`] | one POST to `search.php` per distinct query | identical searches within one query collapse; different ones are issued concurrently |
//!
//! Loaders are built **per GraphQL request** (see [`super::request`]), so their
//! cache is a request-scoped cache: repeating a key inside one query is free,
//! and nothing is held long enough to go stale.
//!
//! # These are not the client's cache
//!
//! [`crate::client`] holds its own process-wide cache of page *bodies*, so the
//! obvious question is what these add. The answer is the parse. A cached body is
//! still 670KB of HTML that `scraper` has to walk into 5,391 structs every time
//! somebody asks; the client can save the refetch, but only a loader can say
//! "you already have this parsed". So the two stack rather than overlap:
//!
//! - the client's memory cache saves the request and the disk read,
//! - the loaders save the parse, and hold the result by `Arc` so a page of
//!   results shares one allocation rather than a clone each.
//!
//! Dropping either one leaves real work being redone.

use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::dataloader::{DataLoader, HashMapCache, Loader};

use super::SharedArchive;
use super::types::RefScheme;
use crate::archive::{SongQuery, paths};
use crate::client::{self, Client};
use crate::error::Error;
use crate::models::{Album, Artist, Book, Label, Song, SongSummary, Source, Track, WaterwaysSong};
use crate::parse;

/// Loader errors are cloned out to every waiter in a batch, so they must be
/// cheap to clone — hence the `Arc`.
pub type LoadError = Arc<Error>;

/// Turn a loader error into a GraphQL error. `Arc<Error>` can't implement
/// `Into<async_graphql::Error>` here (both types are foreign), so resolvers call
/// this explicitly.
pub fn to_gql_error(e: LoadError) -> async_graphql::Error {
    async_graphql::Error::new(e.to_string())
}

/// One page on one archive.
#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct PageKey {
    pub source: Source,
    pub path: String,
}

impl PageKey {
    /// Build a key, dropping any `#fragment`.
    ///
    /// One album page routinely documents several releases and the fragment is
    /// what tells them apart — but it is the same GET. Keeping it in the key
    /// would fetch that page once per release on it.
    pub fn new(source: Source, path: &str) -> Self {
        Self {
            source,
            path: path.split_once('#').map_or(path, |(p, _)| p).to_string(),
        }
    }
}

/// Fetches pages. The one loader that reaches the network.
///
/// A batch is N concurrent GETs, deduplicated by key and throttled by the
/// client's semaphore. This is the loader that earns its keep: without it a
/// query that follows a song into an album and back into its tracks refetches
/// the song page it started from.
pub struct PageLoader(Client);

impl Loader<PageKey> for PageLoader {
    type Value = Arc<String>;
    type Error = LoadError;

    async fn load(&self, keys: &[PageKey]) -> Result<HashMap<PageKey, Arc<String>>, Self::Error> {
        let calls: Vec<_> = keys
            .iter()
            .map(|key| async move { (key.clone(), self.0.get(key.source, &key.path).await) })
            .collect();

        let mut out = HashMap::with_capacity(calls.len());
        for (key, body) in client::fan_out(calls).await {
            match body {
                Ok(html) => {
                    out.insert(key, html);
                }
                // The archive is decades of hand-written HTML with plenty of
                // dead links in it. One 404 must not sink a query that asked
                // for fifty pages; an absent key resolves to null.
                Err(e) => tracing::warn!(path = %key.path, error = %e, "skipping page"),
            }
        }
        Ok(out)
    }
}

pub type Pages = DataLoader<PageLoader, HashMapCache>;

/// Load every key's page through [`PageLoader`] and parse it.
///
/// `parse_one` is sync, which is what keeps `scraper::Html` off the far side of
/// an await — see the warning in [`crate::parse`]. It receives the *full* path
/// including any fragment, because that is what selects a release from a page
/// documenting several, while the fetch went out without it.
async fn parse_each<T>(
    pages: &Pages,
    source: Source,
    keys: &[String],
    parse_one: impl Fn(&str, &str) -> Option<T>,
) -> Result<HashMap<String, T>, LoadError> {
    let wanted: Vec<PageKey> = keys.iter().map(|p| PageKey::new(source, p)).collect();
    let loaded = pages.load_many(wanted.clone()).await?;

    let mut out = HashMap::with_capacity(keys.len());
    for (path, key) in keys.iter().zip(&wanted) {
        let Some(html) = loaded.get(key) else {
            continue;
        };
        match parse_one(html, path) {
            Some(value) => {
                out.insert(path.clone(), value);
            }
            None => tracing::warn!(%path, "page did not parse as the kind of page expected"),
        }
    }
    Ok(out)
}

/// Song pages, keyed by path.
pub struct SongLoader(Arc<Pages>);

impl Loader<String> for SongLoader {
    type Value = Song;
    type Error = LoadError;

    async fn load(&self, keys: &[String]) -> Result<HashMap<String, Song>, Self::Error> {
        parse_each(&self.0, Source::MainlyNorfolk, keys, |html, path| {
            parse::song(html, path).ok()
        })
        .await
    }
}

/// Album pages, keyed by path — the release and its tracklist together, because
/// they come out of one parse of one document.
pub struct AlbumLoader(Arc<Pages>);

impl Loader<String> for AlbumLoader {
    type Value = (Album, Vec<Track>);
    type Error = LoadError;

    async fn load(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, (Album, Vec<Track>)>, Self::Error> {
        parse_each(&self.0, Source::MainlyNorfolk, keys, |html, path| {
            parse::album(html, path).ok()
        })
        .await
    }
}

/// The canonical key for an artist: the directory form, `/martin.carthy/`.
///
/// The archive serves `/martin.carthy/` and `/martin.carthy/index.html` as the
/// same page, and its own markup uses both — a song page's breadcrumb reaches
/// for one, a navigation link for the other. They have to collapse to one key or
/// the same artist enters the graph as two nodes: the loader's dedup quietly
/// stops working, and a recursive query returns copies of one artist that do not
/// compare equal.
pub fn artist_key(path: &str) -> String {
    let path = path.split_once('#').map_or(path, |(p, _)| p);
    let path = path
        .strip_suffix("index.html")
        .or_else(|| path.strip_suffix("index.htm"))
        .unwrap_or(path);
    format!("{}/", path.trim_end_matches('/'))
}

/// Artist index pages, keyed by the directory form of their path — see
/// [`artist_key`].
pub struct ArtistLoader(Arc<Pages>);

impl Loader<String> for ArtistLoader {
    type Value = Artist;
    type Error = LoadError;

    async fn load(&self, keys: &[String]) -> Result<HashMap<String, Artist>, Self::Error> {
        parse_each(&self.0, Source::MainlyNorfolk, keys, |html, path| {
            parse::artist(html, path).ok()
        })
        .await
    }
}

/// Discography pages, keyed by path. Serves both an artist's chronology and a
/// label's catalogue — the archive writes them with the same markup.
pub struct DiscographyLoader(Arc<Pages>);

impl Loader<String> for DiscographyLoader {
    type Value = Arc<Vec<Album>>;
    type Error = LoadError;

    async fn load(&self, keys: &[String]) -> Result<HashMap<String, Arc<Vec<Album>>>, Self::Error> {
        parse_each(&self.0, Source::MainlyNorfolk, keys, |html, path| {
            Some(Arc::new(parse::discography(html, path)))
        })
        .await
    }
}

/// waterwaysongs.info song pages, keyed by path.
pub struct WaterwaysLoader(Arc<Pages>);

impl Loader<String> for WaterwaysLoader {
    type Value = WaterwaysSong;
    type Error = LoadError;

    async fn load(&self, keys: &[String]) -> Result<HashMap<String, WaterwaysSong>, Self::Error> {
        parse_each(&self.0, Source::Waterways, keys, |html, path| {
            parse::waterways_song(html, path).ok()
        })
        .await
    }
}

/// One of the archive's generated whole-list pages.
#[derive(Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub enum IndexKind {
    /// The Child ballads the archive covers.
    Child,
    /// The Laws index — American ballads of British origin.
    Laws,
    /// Every song and tune on the archive. 670KB.
    Songs,
    /// The record labels with a discography, read from the site navigation.
    Labels,
    /// The bibliography — every book the archive's song pages cite.
    Books,
}

impl IndexKind {
    fn path(self) -> &'static str {
        match self {
            IndexKind::Child => paths::CHILD_INDEX,
            IndexKind::Laws => paths::LAWS_INDEX,
            IndexKind::Songs => paths::SONG_INDEX,
            IndexKind::Labels => paths::FOLK,
            IndexKind::Books => paths::BOOKS,
        }
    }
}

/// What an index page parses into.
///
/// Several shapes under one loader because they are the same job — fetch one
/// whole-list page, parse the lot — and sharing the loader is what makes that
/// parse happen once per request however many resolvers ask for it. The
/// bibliography is the case that earns it: every `BookRef` on a song resolves
/// through the same parsed list.
#[derive(Clone)]
pub enum Index {
    Songs(Arc<Vec<SongSummary>>),
    Labels(Arc<Vec<Label>>),
    Books(Arc<Vec<Book>>),
}

impl Index {
    pub fn songs(&self) -> Option<Arc<Vec<SongSummary>>> {
        match self {
            Index::Songs(songs) => Some(songs.clone()),
            _ => None,
        }
    }

    pub fn labels(&self) -> Option<Arc<Vec<Label>>> {
        match self {
            Index::Labels(labels) => Some(labels.clone()),
            _ => None,
        }
    }

    pub fn books(&self) -> Option<Arc<Vec<Book>>> {
        match self {
            Index::Books(books) => Some(books.clone()),
            _ => None,
        }
    }
}

/// The archive's generated indexes.
pub struct IndexLoader(Arc<Pages>);

impl Loader<IndexKind> for IndexLoader {
    type Value = Index;
    type Error = LoadError;

    async fn load(&self, keys: &[IndexKind]) -> Result<HashMap<IndexKind, Index>, Self::Error> {
        // Not `parse_each`: which parser runs depends on the key rather than on
        // the page, and the song index is 670KB — big enough that handing it
        // through a generic closure by value would be a copy worth avoiding.
        let wanted: Vec<PageKey> = keys
            .iter()
            .map(|kind| PageKey::new(Source::MainlyNorfolk, kind.path()))
            .collect();
        let loaded = self.0.load_many(wanted.clone()).await?;

        let mut out = HashMap::with_capacity(keys.len());
        for (kind, key) in keys.iter().zip(&wanted) {
            let Some(html) = loaded.get(key) else {
                continue;
            };
            let index = match kind {
                IndexKind::Labels => Index::Labels(Arc::new(parse::labels(html))),
                IndexKind::Books => Index::Books(Arc::new(parse::books(html, &key.path))),
                _ => Index::Songs(Arc::new(parse::song_list(html, &key.path))),
            };
            out.insert(*kind, index);
        }
        Ok(out)
    }
}

/// A song search, in a form that can be a loader key.
///
/// Mirrors [`SongQuery`], which is neither hashable nor owned in the way a key
/// must be. Kept in sync by [`Self::to_query`] rather than by inheritance.
#[derive(Clone, Default, Hash, PartialEq, Eq, Debug)]
pub struct SongSearch {
    pub title: Option<String>,
    pub author: Option<String>,
    pub scheme: Option<RefScheme>,
    pub number: Option<String>,
}

impl SongSearch {
    pub fn to_query(&self) -> SongQuery {
        SongQuery {
            title: self.title.clone(),
            author: self.author.clone(),
            key: self.scheme.map(Into::into),
            keyval: self.number.clone(),
        }
    }
}

/// Runs the archive's own song search.
///
/// The right way to look a song up: `search.php` answers a title query with 10KB
/// the server has already filtered, where the full index it would otherwise take
/// is 670KB. A batch is one POST per distinct search, concurrently; the same
/// search twice in one query is one POST.
pub struct SearchLoader(SharedArchive);

impl Loader<SongSearch> for SearchLoader {
    type Value = Arc<Vec<SongSummary>>;
    type Error = LoadError;

    async fn load(
        &self,
        keys: &[SongSearch],
    ) -> Result<HashMap<SongSearch, Arc<Vec<SongSummary>>>, Self::Error> {
        let calls: Vec<_> = keys
            .iter()
            .map(|key| async move { (key.clone(), self.0.search_songs(&key.to_query()).await) })
            .collect();

        let mut out = HashMap::with_capacity(calls.len());
        for (key, found) in client::fan_out(calls).await {
            match found {
                Ok(songs) => {
                    out.insert(key, Arc::new(songs));
                }
                Err(e) => tracing::warn!(error = %e, "song search failed"),
            }
        }
        Ok(out)
    }
}

/// Runs the archive's record search, keyed by the query string. Results arrive
/// grouped by credited artist, as the archive groups them.
pub struct RecordsSearchLoader(SharedArchive);

impl Loader<String> for RecordsSearchLoader {
    type Value = Arc<Vec<(String, Vec<Album>)>>;
    type Error = LoadError;

    async fn load(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Arc<Vec<(String, Vec<Album>)>>>, Self::Error> {
        let calls: Vec<_> = keys
            .iter()
            .map(|query| async move { (query.clone(), self.0.search_records(query).await) })
            .collect();

        let mut out = HashMap::with_capacity(calls.len());
        for (query, found) in client::fan_out(calls).await {
            match found {
                Ok(groups) => {
                    out.insert(query, Arc::new(groups));
                }
                Err(e) => tracing::warn!(%query, error = %e, "record search failed"),
            }
        }
        Ok(out)
    }
}

// Loader handles as resolvers see them. `DataLoader::new` only deduplicates
// within a single batch, so these are built `with_cache` to get a genuine
// request-scoped cache: a page pulled for one field is free for every later
// field in the same query.
pub type Songs = DataLoader<SongLoader, HashMapCache>;
pub type Albums = DataLoader<AlbumLoader, HashMapCache>;
pub type Artists = DataLoader<ArtistLoader, HashMapCache>;
pub type Discographies = DataLoader<DiscographyLoader, HashMapCache>;
pub type WaterwaysSongs = DataLoader<WaterwaysLoader, HashMapCache>;
pub type Indexes = DataLoader<IndexLoader, HashMapCache>;
pub type SongSearches = DataLoader<SearchLoader, HashMapCache>;
pub type RecordSearches = DataLoader<RecordsSearchLoader, HashMapCache>;

/// All loaders for one GraphQL request, ready to be attached as request data.
pub struct Loaders {
    pub pages: Arc<Pages>,
    pub songs: Songs,
    pub albums: Albums,
    pub artists: Artists,
    pub discographies: Discographies,
    pub waterways: WaterwaysSongs,
    pub indexes: Indexes,
    pub searches: SongSearches,
    pub record_searches: RecordSearches,
}

/// Wrap a loader with a request-scoped cache. `DataLoader::new` only
/// deduplicates within a single batch, which is not enough here: the graph's
/// cycles mean a page is asked for again several resolver hops later, long after
/// its batch has flushed.
fn request_scoped<L>(loader: L) -> DataLoader<L, HashMapCache> {
    DataLoader::with_cache(loader, tokio::spawn, HashMapCache::default())
}

impl Loaders {
    pub fn new(archive: SharedArchive) -> Self {
        // The page loader is shared rather than duplicated: every other loader
        // needs a page, and going through the same handle means they hit the
        // same request-scoped cache instead of each fetching their own copy.
        // That is what stops the graph's cycles costing anything.
        let pages = Arc::new(Pages::with_cache(
            PageLoader(archive.client().clone()),
            tokio::spawn,
            HashMapCache::default(),
        ));

        Self {
            songs: request_scoped(SongLoader(pages.clone())),
            albums: request_scoped(AlbumLoader(pages.clone())),
            artists: request_scoped(ArtistLoader(pages.clone())),
            discographies: request_scoped(DiscographyLoader(pages.clone())),
            waterways: request_scoped(WaterwaysLoader(pages.clone())),
            indexes: request_scoped(IndexLoader(pages.clone())),
            searches: request_scoped(SearchLoader(archive.clone())),
            record_searches: request_scoped(RecordsSearchLoader(archive)),
            pages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fragment_does_not_make_a_second_fetch() {
        // One album page documents several releases. They must share a key, or
        // the page is fetched once per release listed on it.
        let a = PageKey::new(Source::MainlyNorfolk, "/folk/records/redrice.html#rice");
        let b = PageKey::new(Source::MainlyNorfolk, "/folk/records/redrice.html#carthy");
        assert_eq!(a, b);
        assert_eq!(a.path, "/folk/records/redrice.html");
    }

    #[test]
    fn every_spelling_of_an_artist_path_is_one_key() {
        // The bug this prevents: two spellings, two nodes, and a recursive
        // query handing back the same artist twice without them comparing equal.
        for spelling in [
            "/martin.carthy/",
            "/martin.carthy",
            "/martin.carthy/index.html",
            "/martin.carthy/index.htm",
            "/martin.carthy/#top",
        ] {
            assert_eq!(artist_key(spelling), "/martin.carthy/", "{spelling}");
        }
    }

    #[test]
    fn the_same_path_on_two_archives_is_two_keys() {
        assert_ne!(
            PageKey::new(Source::MainlyNorfolk, "/index.html"),
            PageKey::new(Source::Waterways, "/index.html")
        );
    }

    #[test]
    fn a_search_key_carries_every_field_into_the_form() {
        let form = SongSearch {
            title: Some("reynardine".into()),
            scheme: Some(RefScheme::Child),
            number: Some("2".into()),
            ..Default::default()
        }
        .to_query()
        .to_form();
        assert!(form.contains(&("song", "reynardine".to_string())));
        assert!(form.contains(&("key", "child".to_string())));
        assert!(form.contains(&("keyval", "2".to_string())));
    }

    #[test]
    fn searches_differing_only_by_scheme_are_different_keys() {
        let roud = SongSearch {
            number: Some("12".into()),
            scheme: Some(RefScheme::Roud),
            ..Default::default()
        };
        let child = SongSearch {
            scheme: Some(RefScheme::Child),
            ..roud.clone()
        };
        assert_ne!(roud, child);
    }
}
