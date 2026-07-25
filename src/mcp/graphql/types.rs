//! GraphQL type wrappers around the domain models.
//!
//! Songs, albums, artists and tracks are `#[Object]`s rather than plain structs
//! because most of what makes them interesting is an edge, and on this API an
//! edge is a page fetch. `Recording.album` resolves an album page,
//! `SongSummary.song` a song page, `Track.song` closes the cycle back into the
//! song archive — and a query that asks for none of them fetches none of them.
//!
//! Every one of those edges says what it costs in its own description. That is
//! not decoration: the thing composing the query is usually a model, and it is
//! the only party in a position to decide whether a hundred page fetches are
//! worth it to a hand-maintained site.

use std::sync::Arc;

use async_graphql::{Context, Enum, Object, Result, SimpleObject};

use super::connection::{ListConnection, PageArgs, page_complexity, paginate};
use super::loaders::{
    Albums, Artists, Discographies, Index, IndexKind, Indexes, PageKey, Pages, Songs,
    WaterwaysSongs, to_gql_error,
};
use crate::archive::{RefKey, paths};
use crate::models::{
    Album, Artist, Attribution, BookRef, Label, Link, LyricVersion, Note, Page, Recording, Song,
    SongRefs, SongSummary, Source, Track, WaterwaysSong,
};
use crate::parse;

/// Which archive a record came from. The two sites are unrelated projects with
/// different markup, so the source travels with every record rather than being
/// inferred from the path.
#[derive(Enum, Copy, Clone, Eq, PartialEq, Debug)]
#[graphql(name = "Source")]
pub enum GqlSource {
    /// mainlynorfolk.info — Reinhard Zierke's English folk archive.
    MainlyNorfolk,
    /// waterwaysongs.info — canal and inland-waterways songs.
    Waterways,
}

impl From<Source> for GqlSource {
    fn from(s: Source) -> Self {
        match s {
            Source::MainlyNorfolk => GqlSource::MainlyNorfolk,
            Source::Waterways => GqlSource::Waterways,
        }
    }
}

impl From<GqlSource> for Source {
    fn from(s: GqlSource) -> Self {
        match s {
            GqlSource::MainlyNorfolk => Source::MainlyNorfolk,
            GqlSource::Waterways => Source::Waterways,
        }
    }
}

/// A reference scheme the archive's own search indexes. Mirrors the `key` select
/// on its search form.
#[derive(Enum, Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum RefScheme {
    /// Steve Roud's Folk Song Index. The broadest, and the archive's default.
    Roud,
    /// Francis James Child's numbers. Only the 305 canonical ballads.
    Child,
    /// G. Malcolm Laws' codes for American ballads of British origin, e.g. `P15`.
    Laws,
    /// The Greig-Duncan Folk Song Collection, `volume:number`.
    GreigDuncan,
    /// Sam Henry's Songs of the People, e.g. `H163`.
    Henry,
}

impl From<RefScheme> for RefKey {
    fn from(s: RefScheme) -> Self {
        match s {
            RefScheme::Roud => RefKey::Roud,
            RefScheme::Child => RefKey::Child,
            RefScheme::Laws => RefKey::Laws,
            RefScheme::GreigDuncan => RefKey::GreigDuncan,
            RefScheme::Henry => RefKey::Henry,
        }
    }
}

// ============ Loading through the request's loaders ============

/// Fetch and parse any page. Goes through the page loader, so the HTML is
/// deduplicated across the whole request even where the parse is not.
pub(crate) async fn load_page(
    ctx: &Context<'_>,
    source: Source,
    path: &str,
) -> Result<Option<GqlPage>> {
    let html = ctx
        .data::<Arc<Pages>>()?
        .load_one(PageKey::new(source, path))
        .await
        .map_err(to_gql_error)?;
    Ok(html.and_then(|html| parse::page(&html, path, source).ok().map(GqlPage::from)))
}

/// Fetch and parse one of the archive's song lists at an arbitrary path — an
/// artist's own song index, say. The named whole-archive indexes go through
/// [`load_song_index`] instead, which caches the parse as well as the fetch.
pub(crate) async fn load_song_list(ctx: &Context<'_>, path: &str) -> Result<Vec<GqlSongSummary>> {
    let html = ctx
        .data::<Arc<Pages>>()?
        .load_one(PageKey::new(Source::MainlyNorfolk, path))
        .await
        .map_err(to_gql_error)?;
    Ok(html
        .map(|html| parse::song_list(&html, path))
        .unwrap_or_default()
        .into_iter()
        .map(GqlSongSummary::from)
        .collect())
}

/// One of the archive's generated song indexes.
pub(crate) async fn load_song_index(
    ctx: &Context<'_>,
    kind: IndexKind,
) -> Result<Arc<Vec<SongSummary>>> {
    Ok(load_index(ctx, kind)
        .await?
        .and_then(|i| i.songs())
        .unwrap_or_default())
}

/// The record labels with a discography on the archive.
pub(crate) async fn load_labels(ctx: &Context<'_>) -> Result<Arc<Vec<Label>>> {
    Ok(load_index(ctx, IndexKind::Labels)
        .await?
        .and_then(|i| i.labels())
        .unwrap_or_default())
}

async fn load_index(ctx: &Context<'_>, kind: IndexKind) -> Result<Option<Index>> {
    ctx.data::<Indexes>()?
        .load_one(kind)
        .await
        .map_err(to_gql_error)
}

pub(crate) async fn load_song(ctx: &Context<'_>, path: &str) -> Result<Option<GqlSong>> {
    Ok(ctx
        .data::<Songs>()?
        .load_one(path.to_string())
        .await
        .map_err(to_gql_error)?
        .map(GqlSong::from))
}

pub(crate) async fn load_artist(ctx: &Context<'_>, path: &str) -> Result<Option<GqlArtist>> {
    Ok(ctx
        .data::<Artists>()?
        .load_one(path.to_string())
        .await
        .map_err(to_gql_error)?
        .map(GqlArtist::from))
}

pub(crate) async fn load_album(ctx: &Context<'_>, path: &str) -> Result<Option<GqlAlbum>> {
    Ok(ctx
        .data::<Albums>()?
        .load_one(path.to_string())
        .await
        .map_err(to_gql_error)?
        .map(|(album, _)| GqlAlbum::from(album)))
}

/// A discography page, as albums. Serves an artist's chronology and a label's
/// catalogue alike — the archive writes both the same way.
pub(crate) async fn load_discography(ctx: &Context<'_>, path: &str) -> Result<Vec<GqlAlbum>> {
    Ok(ctx
        .data::<Discographies>()?
        .load_one(path.to_string())
        .await
        .map_err(to_gql_error)?
        .map(|albums| albums.iter().cloned().map(GqlAlbum::from).collect())
        .unwrap_or_default())
}

pub(crate) async fn load_waterways_song(
    ctx: &Context<'_>,
    path: &str,
) -> Result<Option<GqlWaterwaysSong>> {
    Ok(ctx
        .data::<WaterwaysSongs>()?
        .load_one(path.to_string())
        .await
        .map_err(to_gql_error)?
        .map(GqlWaterwaysSong::from))
}

/// The waterwaysongs.info song menu. Titles and paths only — the menu carries
/// nothing else, so every other field comes back empty until the song's own page
/// is fetched.
pub(crate) async fn load_waterways_index(ctx: &Context<'_>) -> Result<Vec<WaterwaysSong>> {
    let html = ctx
        .data::<Arc<Pages>>()?
        .load_one(PageKey::new(Source::Waterways, paths::WATERWAYS_MENU))
        .await
        .map_err(to_gql_error)?;
    Ok(html
        .map(|html| parse::waterways_index(&html))
        .unwrap_or_default())
}

// ============ Plain data ============

/// A link to somewhere off-archive, as it appeared on the page.
#[derive(SimpleObject)]
#[graphql(name = "Link")]
pub struct GqlLink {
    pub text: String,
    pub url: String,
}

impl From<Link> for GqlLink {
    fn from(l: Link) -> Self {
        Self {
            text: l.text,
            url: l.url,
        }
    }
}

/// The reference numbers a song carries.
///
/// Each is a list because the archive genuinely lists several for one song — a
/// ballad Roud split and Child didn't, or a page covering a family of related
/// songs. Values are kept as the archive writes them (`"P15"`, `"2:329"`),
/// which is also what its own search accepts back.
#[derive(SimpleObject)]
#[graphql(name = "SongRefs")]
pub struct GqlSongRefs {
    /// Steve Roud's Folk Song Index, e.g. `"12"`.
    pub roud: Vec<String>,
    /// Francis James Child's number, e.g. `"2"`.
    pub child: Vec<String>,
    /// G. Malcolm Laws' code, e.g. `"P15"`.
    pub laws: Vec<String>,
    /// Greig-Duncan, `"volume:number"`.
    pub greig_duncan: Vec<String>,
    /// Sam Henry's Songs of the People, e.g. `"H163"`.
    pub henry: Vec<String>,
    /// The Ballad Index code, e.g. `"C002"`.
    pub ballad_index: Option<String>,
    /// Vaughan Williams Memorial Library record ids.
    pub vwml: Vec<String>,
    /// The archive's canonical name for a song that circulates under many. The
    /// best single key for deduplicating.
    pub master_title: Option<String>,
}

impl From<SongRefs> for GqlSongRefs {
    fn from(r: SongRefs) -> Self {
        Self {
            roud: r.roud,
            child: r.child,
            laws: r.laws,
            greig_duncan: r.greig_duncan,
            henry: r.henry,
            ballad_index: r.ballad_index,
            vwml: r.vwml,
            master_title: r.master_title,
        }
    }
}

/// One performer's set of lyrics. Song pages carry a block per performer,
/// because performers sing materially different words.
#[derive(SimpleObject)]
#[graphql(name = "LyricVersion")]
pub struct GqlLyricVersion {
    /// Anchor on the page, without the `#`.
    pub anchor: String,
    /// Whose version this is, where the heading says.
    pub performer: Option<String>,
    /// The lyrics, line breaks preserved.
    pub text: String,
}

impl From<LyricVersion> for GqlLyricVersion {
    fn from(l: LyricVersion) -> Self {
        Self {
            anchor: l.anchor,
            performer: l.performer,
            text: l.text,
        }
    }
}

/// A quoted note — sleeve notes, a collector's commentary, a booklet essay.
/// These are the archive's real scholarly value, and they are always quotations,
/// so the attribution stays apart from the text.
#[derive(SimpleObject)]
#[graphql(name = "Note")]
pub struct GqlNote {
    /// Who is being quoted, where the surrounding prose names them.
    pub attribution: Option<String>,
    pub text: String,
}

impl From<Note> for GqlNote {
    fn from(n: Note) -> Self {
        Self {
            attribution: n.attribution,
            text: n.text,
        }
    }
}

/// A book cited in a song page's bibliography.
#[derive(SimpleObject)]
#[graphql(name = "BookRef")]
pub struct GqlBookRef {
    pub author: Option<String>,
    pub title: String,
    /// Path to the archive's own page for the book, when it has one. Read it
    /// with `page(path:)`.
    pub path: Option<String>,
}

impl From<BookRef> for GqlBookRef {
    fn from(b: BookRef) -> Self {
        Self {
            author: b.author,
            title: b.title,
            path: b.path,
        }
    }
}

/// Any archive page as title, plain text and links. The escape hatch, so nothing
/// on either site is unreachable for want of a richer type.
#[derive(SimpleObject)]
#[graphql(name = "Page")]
pub struct GqlPage {
    pub path: String,
    pub title: String,
    /// Main content as plain text, markup stripped and whitespace collapsed.
    pub text: String,
    /// Every in-archive link on the page.
    pub links: Vec<GqlLink>,
    pub source: GqlSource,
}

impl From<Page> for GqlPage {
    fn from(p: Page) -> Self {
        Self {
            path: p.path,
            title: p.title,
            text: p.text,
            links: p.links.into_iter().map(GqlLink::from).collect(),
            source: p.source.into(),
        }
    }
}

/// A canal or inland-waterways song from waterwaysongs.info.
///
/// Its own type rather than a `Song`: that site is a different project with no
/// Roud/Child apparatus, and folding it into `Song` would mean a record whose
/// every reference field is permanently empty.
#[derive(SimpleObject)]
#[graphql(name = "WaterwaysSong")]
pub struct GqlWaterwaysSong {
    /// Path on waterwaysongs.info. The id — pass it to `waterwaysSong(path:)`.
    pub path: String,
    pub title: String,
    /// Writer or collector, from the page's author line.
    pub author: Option<String>,
    pub lyrics: Option<String>,
    pub notes: Vec<String>,
    /// Recordings the page lists, as free text.
    pub recorded_by: Vec<String>,
    /// Absolute URL of an audio file the page embeds.
    pub audio_url: Option<String>,
}

impl From<WaterwaysSong> for GqlWaterwaysSong {
    fn from(s: WaterwaysSong) -> Self {
        Self {
            path: s.path,
            title: s.title,
            author: s.author,
            lyrics: s.lyrics,
            notes: s.notes,
            recorded_by: s.recorded_by,
            audio_url: s.audio_url,
        }
    }
}

/// One credited artist's matching releases, as the record search groups them.
#[derive(SimpleObject)]
#[graphql(name = "ArtistRecords")]
pub struct GqlArtistRecords {
    /// Credited artist, as the search results name them. Not a path — follow
    /// an album's own fields, or `artist(path:)`, to reach the artist page.
    pub artist: String,
    pub albums: Vec<GqlAlbum>,
}

// ============ The graph ============

/// A song or tune page. Everything on it arrived in one fetch except
/// `attributions { artist }` and `recordings { album }`, which are edges.
pub struct GqlSong(pub Song);

impl From<Song> for GqlSong {
    fn from(s: Song) -> Self {
        Self(s)
    }
}

#[Object(name = "Song")]
impl GqlSong {
    /// Site-absolute path. This is the id: it is stable, it is what every link
    /// on the site uses, and it is what `song(path:)` takes.
    async fn path(&self) -> &str {
        &self.0.path
    }
    /// The page's heading, verbatim — often several titles joined by `/`.
    async fn title(&self) -> &str {
        &self.0.title
    }
    /// `title` split into the alternate names this one song goes by.
    async fn titles(&self) -> &[String] {
        &self.0.titles
    }
    async fn refs(&self) -> GqlSongRefs {
        self.0.refs.clone().into()
    }
    /// True when the reference block says `trad.`
    async fn traditional(&self) -> bool {
        self.0.traditional
    }
    async fn source(&self) -> GqlSource {
        self.0.source.into()
    }
    /// One block per performer. Free — they arrive with the page.
    async fn lyrics(&self) -> Vec<GqlLyricVersion> {
        self.0.lyrics.iter().cloned().map(Into::into).collect()
    }
    /// Sleeve notes and commentary quoted on the page. Free.
    async fn notes(&self) -> Vec<GqlNote> {
        self.0.notes.iter().cloned().map(Into::into).collect()
    }
    /// Books cited in the page's bibliography. Free.
    async fn bibliography(&self) -> Vec<GqlBookRef> {
        self.0
            .bibliography
            .iter()
            .cloned()
            .map(Into::into)
            .collect()
    }
    /// Off-archive links from the reference block — VWML, Ballad Index, Mudcat.
    async fn external_links(&self) -> Vec<GqlLink> {
        self.0
            .external_links
            .iter()
            .cloned()
            .map(Into::into)
            .collect()
    }

    /// Every artist whose section of the site claims this page.
    ///
    /// Free — parsed from the breadcrumb that arrived with the song. A page
    /// routinely serves several artists, each calling the song something
    /// different.
    async fn attributions(&self) -> Vec<GqlAttribution> {
        self.0
            .attributions
            .iter()
            .cloned()
            .map(Into::into)
            .collect()
    }

    /// Recordings cited in the page's prose.
    ///
    /// Free — the archive writes them as sentences on this page, and they were
    /// parsed out of the fetch you already paid for. Following `album` on one of
    /// them is what costs.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn recordings(
        &self,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlRecording>> {
        let recordings: Vec<GqlRecording> = self
            .0
            .recordings
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, r)| GqlRecording::at(i, r))
            .collect();
        paginate(
            recordings,
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |r| r.cursor.clone(),
        )
    }
}

/// A song as it appears in an index or a search result: a title, a path and the
/// reference numbers. Deliberately not a `Song` — filling in the rest means one
/// page fetch per entry.
pub struct GqlSongSummary(pub SongSummary);

impl From<SongSummary> for GqlSongSummary {
    fn from(s: SongSummary) -> Self {
        Self(s)
    }
}

#[Object(name = "SongSummary")]
impl GqlSongSummary {
    /// Site-absolute path. The id — pass it to `song(path:)`.
    async fn path(&self) -> &str {
        &self.0.path
    }
    /// Full title line, `/`-joined alternates included.
    async fn title(&self) -> &str {
        &self.0.title
    }
    async fn titles(&self) -> &[String] {
        &self.0.titles
    }
    async fn refs(&self) -> GqlSongRefs {
        self.0.refs.clone().into()
    }
    async fn source(&self) -> GqlSource {
        self.0.source.into()
    }

    /// The full song page — lyrics, notes, recordings, bibliography.
    ///
    /// **Fetches one page per summary in the result.** Selecting this on a
    /// 200-entry index is 200 requests, issued four at a time at a site one
    /// person maintains; it will be slow and it is rude. Narrow the list first,
    /// or page through it, and select this only where you actually need the
    /// song itself.
    #[graphql(complexity = "10 + child_complexity")]
    async fn song(&self, ctx: &Context<'_>) -> Result<Option<GqlSong>> {
        load_song(ctx, &self.0.path).await
    }
}

/// One artist's claim on a song page, from the breadcrumb.
pub struct GqlAttribution(pub Attribution);

impl From<Attribution> for GqlAttribution {
    fn from(a: Attribution) -> Self {
        Self(a)
    }
}

#[Object(name = "Attribution")]
impl GqlAttribution {
    /// Artist display name, e.g. `"Eliza Carthy & Nancy Kerr"`.
    async fn name(&self) -> &str {
        &self.0.artist
    }
    /// Path to the artist's index page. The id — pass it to `artist(path:)`.
    async fn path(&self) -> &str {
        &self.0.artist_path
    }
    /// What this artist calls the song, which need not be the page's own title.
    async fn title(&self) -> &str {
        &self.0.title
    }

    /// The artist themselves. **Fetches this artist's index page**, one per
    /// attribution selected.
    #[graphql(complexity = "10 + child_complexity")]
    async fn artist(&self, ctx: &Context<'_>) -> Result<Option<GqlArtist>> {
        load_artist(ctx, &self.0.artist_path).await
    }
}

/// A recording of a song, as cited in a song page's prose.
///
/// The archive writes these as sentences ("A.L. Lloyd sang it in 1956 on …"), so
/// the structured fields are what survives extraction and `context` is the
/// sentence they came from.
pub struct GqlRecording {
    recording: Recording,
    /// Recordings have no id of their own. Their position in the page's prose
    /// is stable — the list arrives whole, in document order — so that is the
    /// cursor.
    cursor: String,
}

impl GqlRecording {
    fn at(index: usize, recording: Recording) -> Self {
        Self {
            recording,
            cursor: index.to_string(),
        }
    }
}

#[Object(name = "Recording")]
impl GqlRecording {
    /// Performer, as the prose names them.
    async fn performer(&self) -> Option<&str> {
        self.recording.performer.as_deref()
    }
    /// Album title, as cited.
    async fn album_title(&self) -> Option<&str> {
        self.recording.album_title.as_deref()
    }
    /// Path to the album page, `#fragment` included. Free — follow `album` to
    /// fetch it, or pass this to `album(path:)`.
    async fn album_path(&self) -> Option<&str> {
        self.recording.album_path.as_deref()
    }
    /// Year, where the prose states one.
    async fn year(&self) -> Option<&str> {
        self.recording.year.as_deref()
    }
    /// The sentence this was extracted from, so you can see what the structured
    /// fields flattened away.
    async fn context(&self) -> &str {
        &self.recording.context
    }

    /// The release. **Fetches this album's page**, one per recording selected.
    /// Null when the prose cited no link to one.
    #[graphql(complexity = "10 + child_complexity")]
    async fn album(&self, ctx: &Context<'_>) -> Result<Option<GqlAlbum>> {
        match &self.recording.album_path {
            Some(path) => load_album(ctx, path).await,
            None => Ok(None),
        }
    }
}

/// An artist, band, or tradition bearer with a section of the archive.
pub struct GqlArtist(pub Artist);

impl From<Artist> for GqlArtist {
    fn from(a: Artist) -> Self {
        Self(a)
    }
}

#[Object(name = "Artist")]
impl GqlArtist {
    /// Path to the artist's index, e.g. `"/martin.carthy/"`. The id.
    async fn path(&self) -> &str {
        &self.0.path
    }
    async fn name(&self) -> &str {
        &self.0.name
    }
    async fn source(&self) -> GqlSource {
        self.0.source.into()
    }

    /// The artist's biography. **Fetches the biography page** (one request).
    /// Null when the archive has none for them.
    #[graphql(complexity = "10 + child_complexity")]
    async fn biography(&self, ctx: &Context<'_>) -> Result<Option<GqlPage>> {
        match &self.0.biography_path {
            Some(path) => load_page(ctx, self.0.source, path).await,
            None => Ok(None),
        }
    }

    /// Chronological discography.
    ///
    /// **Fetches the discography page** — one request, however many releases are
    /// on it, and `totalCount` is then free. Selecting `tracks` on the results
    /// is a further request each.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn discography(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlAlbum>> {
        let albums = match &self.0.discography_path {
            Some(path) => load_discography(ctx, path).await?,
            None => Vec::new(),
        };
        paginate(
            albums,
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |a| a.0.path.clone(),
        )
    }

    /// The songs filed under this artist. **Fetches their song index** — one
    /// request, however many songs. Each result's `song` edge is a further
    /// request.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn songs(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlSongSummary>> {
        let songs = match &self.0.songs_path {
            Some(path) => load_song_list(ctx, path).await?,
            None => Vec::new(),
        };
        paginate(
            songs,
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |s| s.0.path.clone(),
        )
    }
}

/// One release. Discography entries and search results carry the metadata but
/// no tracklist — `tracks` is the edge that fetches the release's own page.
pub struct GqlAlbum(pub Album);

impl From<Album> for GqlAlbum {
    fn from(a: Album) -> Self {
        Self(a)
    }
}

#[Object(name = "Album")]
impl GqlAlbum {
    /// Path to the album page, `#fragment` included — one page often documents
    /// several releases, and the fragment is what distinguishes them. The id.
    async fn path(&self) -> &str {
        &self.0.path
    }
    async fn title(&self) -> &str {
        &self.0.title
    }
    /// Credited artist, e.g. `"Various Artists"`. A name as printed, not a path.
    async fn artist(&self) -> Option<&str> {
        self.0.artist.as_deref()
    }
    async fn year(&self) -> Option<&str> {
        self.0.year.as_deref()
    }
    /// Label name, e.g. `"Topic"`.
    async fn label(&self) -> Option<&str> {
        self.0.label.as_deref()
    }
    /// Catalogue number, e.g. `"12TS340"`.
    async fn catalogue_number(&self) -> Option<&str> {
        self.0.catalogue_number.as_deref()
    }
    /// `"LP"`, `"CD"`, `"EP"`, `"cassette"`, as written.
    async fn format(&self) -> Option<&str> {
        self.0.format.as_deref()
    }
    async fn cover_url(&self) -> Option<&str> {
        self.0.cover_url.as_deref()
    }
    async fn source(&self) -> GqlSource {
        self.0.source.into()
    }

    /// The tracklist. **Fetches this album's page**, one request per album.
    ///
    /// Free only where the album was reached by `album(path:)`, which fetched
    /// that page already. On a discography or a record search it is a request
    /// each, so selecting it across a page of releases is a page of requests.
    #[graphql(complexity = "10 + child_complexity")]
    async fn tracks(&self, ctx: &Context<'_>) -> Result<Vec<GqlTrack>> {
        Ok(ctx
            .data::<Albums>()?
            .load_one(self.0.path.clone())
            .await
            .map_err(to_gql_error)?
            .map(|(_, tracks)| tracks.into_iter().map(GqlTrack::from).collect())
            .unwrap_or_default())
    }
}

/// A track on an album page.
pub struct GqlTrack(pub Track);

impl From<Track> for GqlTrack {
    fn from(t: Track) -> Self {
        Self(t)
    }
}

#[Object(name = "Track")]
impl GqlTrack {
    /// 1-based position as printed, where the page numbers its tracks.
    async fn position(&self) -> Option<u32> {
        self.0.position
    }
    async fn title(&self) -> &str {
        &self.0.title
    }
    /// Playing time as printed, e.g. `"3:42"`.
    async fn duration(&self) -> Option<&str> {
        self.0.duration.as_deref()
    }

    /// The song page this track links to. **Fetches that page**, one per track
    /// selected — a full tracklist is a dozen or more requests.
    ///
    /// This is the edge that closes the cycle: song → recordings → album →
    /// tracks → song. Null where the tracklist gave no link.
    #[graphql(complexity = "10 + child_complexity")]
    async fn song(&self, ctx: &Context<'_>) -> Result<Option<GqlSong>> {
        match &self.0.song_path {
            Some(path) => load_song(ctx, path).await,
            None => Ok(None),
        }
    }
}

/// A record label with a discography page on the archive.
pub struct GqlLabel(pub Label);

impl From<Label> for GqlLabel {
    fn from(l: Label) -> Self {
        Self(l)
    }
}

#[Object(name = "Label")]
impl GqlLabel {
    /// Slug from the path, e.g. `"topic"`.
    async fn id(&self) -> &str {
        &self.0.id
    }
    /// Path to the label's discography page.
    async fn path(&self) -> &str {
        &self.0.path
    }
    /// Display name as the navigation gives it.
    async fn name(&self) -> &str {
        &self.0.name
    }

    /// The label's catalogue. **Fetches the label's discography page** — one
    /// request per label, however many releases are on it. Selecting this
    /// across every label on `labels` is one request per label.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn albums(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlAlbum>> {
        paginate(
            load_discography(ctx, &self.0.path).await?,
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |a| a.0.path.clone(),
        )
    }
}
