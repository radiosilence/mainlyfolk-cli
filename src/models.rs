//! Domain types shared by the parsers, the CLI, and the GraphQL layer.
//!
//! Everything here is what the archive actually publishes, normalised. The
//! archive is a hand-written HTML site, so absence is the norm: almost every
//! field is optional and a parser that finds nothing returns an empty
//! collection rather than an error. Only a page that isn't the kind of page we
//! asked for is a [`crate::error::Error::Parse`].

use serde::{Deserialize, Serialize};

/// Which archive a record came from. The two sites are unrelated projects with
/// completely different markup, so the parsers are separate and the source is
/// carried on every record rather than inferred from the path at use sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// mainlynorfolk.info — Reinhard Zierke's English folk archive.
    MainlyNorfolk,
    /// waterwaysongs.info — canal and inland-waterways songs.
    Waterways,
}

impl Source {
    /// Origin, without a trailing slash.
    pub fn base_url(self) -> &'static str {
        match self {
            Source::MainlyNorfolk => "https://www.mainlynorfolk.info",
            Source::Waterways => "https://www.waterwaysongs.info",
        }
    }

    /// The source owning `url`, if it is one of ours.
    pub fn of_url(url: &str) -> Option<Source> {
        [Source::MainlyNorfolk, Source::Waterways]
            .into_iter()
            .find(|s| url.starts_with(s.base_url()))
    }
}

/// A link to somewhere off-archive, as it appeared on the page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub text: String,
    pub url: String,
}

/// The reference numbers a song carries. Each is a `Vec` because the archive
/// genuinely lists several for one song — a ballad that Roud split and Child
/// didn't, or a page covering a family of related songs.
///
/// Values are kept as the archive writes them (`"P15"`, `"2:329"`, `"H163"`)
/// rather than parsed into numbers: the schemes aren't all numeric, and the
/// string is what the site's own search accepts back.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SongRefs {
    /// Steve Roud's Folk Song Index, e.g. `"12"`.
    pub roud: Vec<String>,
    /// Francis James Child's number, e.g. `"2"`. Only the 305 canonical ballads.
    pub child: Vec<String>,
    /// G. Malcolm Laws' code for American ballads of British origin, e.g. `"P15"`.
    pub laws: Vec<String>,
    /// Greig-Duncan Folk Song Collection, `"volume:number"`, e.g. `"2:329"`.
    pub greig_duncan: Vec<String>,
    /// Sam Henry's Songs of the People, e.g. `"H163"`.
    pub henry: Vec<String>,
    /// The Ballad Index code, e.g. `"C002"`.
    pub ballad_index: Option<String>,
    /// Vaughan Williams Memorial Library record ids, e.g. `"CJS2/10/2868"`.
    pub vwml: Vec<String>,
    /// Steve Gardham's "master title" — the archive's canonical name for a song
    /// that circulates under many. The best single key for deduplicating.
    pub master_title: Option<String>,
}

impl SongRefs {
    /// Whether any reference at all was found. A song page with none is usually
    /// a modern composition rather than a parse failure.
    pub fn is_empty(&self) -> bool {
        self.roud.is_empty()
            && self.child.is_empty()
            && self.laws.is_empty()
            && self.greig_duncan.is_empty()
            && self.henry.is_empty()
            && self.ballad_index.is_none()
            && self.vwml.is_empty()
    }
}

/// One artist's claim on a song page, from the breadcrumb.
///
/// Song pages live under whichever artist the archive filed them beneath, but a
/// page routinely serves several — `theelfinknight.html` sits under Martin
/// Carthy and is also Shirley Collins' "Scarborough Fair". Each breadcrumb line
/// is one of these, so a song knows every artist it belongs to and what each
/// calls it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    /// Artist display name, e.g. `"Eliza Carthy & Nancy Kerr"`.
    pub artist: String,
    /// Path to the artist's index page, e.g. `"/eliza.carthy/"`.
    pub artist_path: String,
    /// What this artist calls the song, which need not be the page's own title.
    pub title: String,
}

/// A set of lyrics on a song page.
///
/// Song pages carry one block per performer, each at its own anchor
/// (`#allloyd`), because performers sing materially different words. The prose
/// links to them as `<a class="lyrics" href="#allloyd">`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LyricVersion {
    /// Anchor without the `#`, e.g. `"allloyd"`. Stable, and what the page's own
    /// prose links to.
    pub anchor: String,
    /// Whose version this is, as the heading gives it.
    pub performer: Option<String>,
    /// The lyrics, with line breaks preserved and entities decoded.
    pub text: String,
}

/// A quoted note — sleeve notes, a collector's commentary, a booklet essay.
///
/// These are the archive's real scholarly value and they are always quotations,
/// so the attribution is kept apart from the text rather than folded in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Who is being quoted, where the surrounding prose names them.
    pub attribution: Option<String>,
    pub text: String,
}

/// A recording of a song, as cited in a song page's prose.
///
/// The archive writes these as sentences ("A.L. Lloyd sang *Scarborough Fair*
/// in 1956 on ..."), so this is what survives structured extraction. `album_path`
/// is the edge worth following — it resolves to a full [`Album`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    /// Performer as the prose names them.
    pub performer: Option<String>,
    /// Album title as cited.
    pub album_title: Option<String>,
    /// Path to the album page, including any `#fragment`.
    pub album_path: Option<String>,
    /// Year of the recording or release, where the prose states one.
    pub year: Option<String>,
    /// The sentence this was extracted from, so a caller can see the context
    /// the structured fields flattened away.
    pub context: String,
}

/// A book cited in a song page's bibliography block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BookRef {
    pub author: Option<String>,
    pub title: String,
    /// Path to the archive's page for the book, when it has one.
    pub path: Option<String>,
}

/// A song or tune page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    /// Site-absolute path, e.g. `"/martin.carthy/songs/theelfinknight.html"`.
    /// This is the id: it is stable, and it is what every link on the site uses.
    pub path: String,
    /// The page's `<h1>`, verbatim — often several titles joined by `/`.
    pub title: String,
    /// `title` split on `/`, trimmed. The alternate names one song goes by.
    pub titles: Vec<String>,
    pub refs: SongRefs,
    /// Every artist whose section of the site claims this page.
    pub attributions: Vec<Attribution>,
    pub lyrics: Vec<LyricVersion>,
    pub notes: Vec<Note>,
    pub recordings: Vec<Recording>,
    pub bibliography: Vec<BookRef>,
    /// Off-archive links from the reference block — VWML, Ballad Index, Mudcat.
    pub external_links: Vec<Link>,
    /// True when the reference block says `trad.`.
    pub traditional: bool,
    pub source: Source,
}

/// A song as it appears in one of the generated indexes or a search result.
///
/// Deliberately not a [`Song`]: the index gives a title, a path and the
/// reference numbers and nothing else, and fetching every page to fill in the
/// rest would be thousands of requests. Callers that want the rest follow
/// `path`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SongSummary {
    pub path: String,
    /// Full title line, `/`-joined alternates included.
    pub title: String,
    pub titles: Vec<String>,
    pub refs: SongRefs,
    pub source: Source,
}

/// An artist, band, or tradition bearer with a section of the archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    /// Path to the artist's index, e.g. `"/martin.carthy/"`. The id.
    pub path: String,
    pub name: String,
    /// Path to the biography page, where one exists.
    pub biography_path: Option<String>,
    /// Path to the chronological discography.
    pub discography_path: Option<String>,
    /// Path to the artist's song index.
    pub songs_path: Option<String>,
    pub source: Source,
}

/// One release in a discography.
///
/// Discography pages group releases under `<h2>` year headings and write each
/// as a sentence carrying label, catalogue number and format. All of it is
/// optional because plenty of entries omit some.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    /// Path to the album page, `#fragment` included — one page often documents
    /// several releases, and the fragment is what distinguishes them.
    pub path: String,
    pub title: String,
    /// Credited artist, e.g. `"Various Artists"` or `"The Thamesiders and Davy Graham"`.
    pub artist: Option<String>,
    /// Year, from the entry or its `<h2>` heading.
    pub year: Option<String>,
    /// Label name, e.g. `"Topic"`.
    pub label: Option<String>,
    /// Catalogue number, e.g. `"12TS340"`.
    pub catalogue_number: Option<String>,
    /// `"LP"`, `"CD"`, `"EP"`, `"cassette"`, as written.
    pub format: Option<String>,
    /// Cover image URL, where the entry has a thumbnail.
    pub cover_url: Option<String>,
    pub source: Source,
}

/// A track on an album page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// 1-based position as printed, where the page numbers its tracks.
    pub position: Option<u32>,
    pub title: String,
    /// Path to the song page, when the track links to one. The edge from a
    /// release back into the song archive.
    pub song_path: Option<String>,
    /// Playing time as printed, e.g. `"3:42"`.
    pub duration: Option<String>,
}

/// A book in the archive's bibliography.
///
/// Distinct from [`BookRef`], which is a citation *on a song page*. This is the
/// bibliography's own entry, with the publication detail a citation omits — and
/// it is what a `BookRef.path` resolves to, which is what turns a cited title
/// into something you can actually read about.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    /// The entry's anchor id, e.g. `"baringgould:songsofthewest"`. Stable, and
    /// what song-page citations link to as `/folk/books/#<id>`.
    pub id: String,
    /// Path to the book's own page, or to its anchor in the bibliography.
    pub path: String,
    pub title: String,
    /// Authors and editors, as the entry credits them.
    pub authors: Vec<String>,
    /// Publisher and place, e.g. `"London: Chatto & Windus Piccadilly"`.
    pub publisher: Option<String>,
    pub year: Option<String>,
    /// Which of the bibliography's sections this sits under — "Ballads and
    /// Songs", "Folk Song and Music", "Biographies", "Other Books".
    pub section: Option<String>,
    /// A full text online, where the archive links one (Gutenberg, archive.org).
    pub online_url: Option<String>,
    pub cover_url: Option<String>,
}

/// A record label with a discography page on the archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    /// Slug from the path, e.g. `"topic"`. The id.
    pub id: String,
    /// Path to the label's discography page.
    pub path: String,
    /// Display name as the navigation gives it, e.g. `"Topic Discography: Records"`.
    pub name: String,
}

/// A canal or inland-waterways song from waterwaysongs.info.
///
/// Its own type rather than a [`Song`]: the site is a different project with no
/// Roud/Child apparatus, and flattening it into `Song` would mean a record whose
/// every reference field is permanently empty.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WaterwaysSong {
    /// Path on waterwaysongs.info, e.g. `"/Songs/H/hard_working.htm"`.
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

/// A page the archive publishes that has no richer type — an essay, an about
/// page, a book entry. The escape hatch, so nothing on the site is unreachable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub path: String,
    pub title: String,
    /// Main content as plain text, markup stripped and whitespace collapsed.
    pub text: String,
    /// Every in-archive link on the page.
    pub links: Vec<Link>,
    pub source: Source,
}

/// Uniform JSON envelope for CLI output: `{"success": true, "data": {...}}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl<T: Serialize> Output<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            message: None,
        }
    }

    pub fn success_msg(message: impl Into<String>) -> Self {
        Self {
            success: true,
            data: None,
            error: None,
            message: Some(message.into()),
        }
    }

    pub fn error(err: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(err.into()),
            message: None,
        }
    }

    pub fn print(&self) {
        match serde_json::to_string_pretty(self) {
            Ok(json) => println!("{json}"),
            Err(e) => eprintln!("{{\"success\":false,\"error\":\"Serialization failed: {e}\"}}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_is_recognised_from_a_url() {
        assert_eq!(
            Source::of_url("https://www.mainlynorfolk.info/folk/"),
            Some(Source::MainlyNorfolk)
        );
        assert_eq!(
            Source::of_url("https://www.waterwaysongs.info/Songs/A/alice_white.htm"),
            Some(Source::Waterways)
        );
        assert_eq!(Source::of_url("https://example.com/folk/"), None);
    }

    #[test]
    fn refs_with_only_a_master_title_are_empty() {
        // A master title is the archive's own naming, not a scholarly reference,
        // so it alone doesn't make a song "referenced".
        let refs = SongRefs {
            master_title: Some("The Elfin Knight".into()),
            ..Default::default()
        };
        assert!(refs.is_empty());
    }

    #[test]
    fn output_error_carries_no_data() {
        let output: Output<()> = Output::error("nope");
        assert!(!output.success);
        assert!(output.data.is_none());
        assert_eq!(output.error.as_deref(), Some("nope"));
    }
}
