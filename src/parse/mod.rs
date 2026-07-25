//! HTML → models. Pure functions, no I/O.
//!
//! Split from [`crate::archive`] so every parser can be tested against a saved
//! fixture without a network, which is the only way to have any confidence
//! against a hand-written site that changes shape page by page.
//!
//! # Two rules every parser here follows
//!
//! **Absence is not an error.** The archive is decades of hand-written HTML;
//! a song page with no lyrics, an album entry with no catalogue number and a
//! discography year with no releases are all normal. Return an empty
//! collection or `None`. Reserve [`crate::error::Error::Parse`] for a document
//! that is not the kind of page that was asked for at all — no `<h1>`, no
//! `#mainArticle` — because that means a moved page or a redirect to an error
//! page, which a caller must be able to distinguish from a sparse one.
//!
//! **`scraper::Html` must never be held across an `.await`.** It is not `Send`,
//! and async-graphql requires `Send` futures — holding one across an await
//! point produces a compiler error whose message points at the resolver rather
//! than at the parse, and it is a miserable half-hour. Parsers being sync
//! functions taking `&str` and returning owned models is what prevents it:
//! keep them that way, and let the caller `await` the fetch first.

use crate::error::Result;
use crate::models::{
    Album, Artist, Book, Label, Page, Song, SongSummary, Source, Track, WaterwaysSong,
};

pub mod album;
pub mod artist;
pub mod book;
pub mod index;
pub mod page;
pub mod song;
pub mod waterways;

/// A song page: titles, references, lyrics, notes, recordings, bibliography.
///
/// `path` is the site-absolute path the HTML came from, needed to resolve the
/// page's relative links into ids.
pub fn song(html: &str, path: &str) -> Result<Song> {
    song::parse(html, path)
}

/// A list of songs, from any of the archive's generated song lists.
///
/// The Child index, the Laws index, the full song index and the `search.php`
/// results are all the same markup — `ul.plain > li > a` followed by
/// `span.comment` carrying the reference numbers — so they are one parser
/// rather than four that drift apart.
pub fn song_list(html: &str, base: &str) -> Vec<SongSummary> {
    index::songs(html, base)
}

/// The archive's record-label discographies, from the site navigation.
///
/// Read from the nav rather than hardcoded: the previous implementation carried
/// a fixed list of eleven labels and the site publishes twenty.
pub fn labels(html: &str) -> Vec<Label> {
    index::labels(html)
}

/// The archive's bibliography — the books its song pages cite.
///
/// Song-page citations link here by anchor, so parsing the bibliography is what
/// turns a cited title into a record with a publisher, a year, and sometimes a
/// link to the full text.
pub fn books(html: &str, base: &str) -> Vec<Book> {
    book::index(html, base)
}

/// An artist's index page.
pub fn artist(html: &str, path: &str) -> Result<Artist> {
    artist::parse(html, path)
}

/// A chronological discography page: `<h2>` year headings over release entries.
pub fn discography(html: &str, base: &str) -> Vec<Album> {
    artist::discography(html, base)
}

/// Results of `records/search.php`, grouped as the archive groups them: one
/// credited artist, then their matching releases.
pub fn records_search(html: &str, base: &str) -> Vec<(String, Vec<Album>)> {
    artist::records_search(html, base)
}

/// An album page: the release itself and its tracks.
///
/// `path` may carry a `#fragment`, and when it does only that release's section
/// is parsed — one page routinely documents several.
pub fn album(html: &str, path: &str) -> Result<(Album, Vec<Track>)> {
    album::parse(html, path)
}

/// Any archive page, as title, plain text and links. The escape hatch, so
/// nothing on the site is unreachable just because it has no richer type.
pub fn page(html: &str, path: &str, source: Source) -> Result<Page> {
    page::parse(html, path, source)
}

/// A waterwaysongs.info song page.
pub fn waterways_song(html: &str, path: &str) -> Result<WaterwaysSong> {
    waterways::song(html, path)
}

/// The waterwaysongs.info song menu. Titles and paths only — the menu carries
/// nothing else, so every other field is left empty.
pub fn waterways_index(html: &str) -> Vec<WaterwaysSong> {
    waterways::index(html)
}

/// Collapse runs of whitespace and trim. The archive's markup is indented for
/// humans, so raw `text()` is full of newlines and runs of spaces that are not
/// meaningful.
pub fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a reference value is a real one.
///
/// The archive writes `Roud -` for a song it has no number for, and a bare `-`
/// stored as a Roud number is worse than storing nothing: it looks like data,
/// and `Song.sameRoud` feeds it straight back into the archive's own search,
/// where it matches nothing. A real reference always carries a digit or a
/// letter.
pub fn is_reference_value(value: &str) -> bool {
    value.chars().any(|c| c.is_ascii_alphanumeric())
}

/// Split a `/`-joined title line into its alternates.
///
/// The archive writes one song's several names as
/// `"The Elfin Knight / Scarborough Fair / Whittingham Fair"`. Splitting on a
/// bare `/` is wrong for titles that contain one, so this splits on a slash
/// with whitespace either side, which is how the archive writes the separator.
pub fn split_titles(title: &str) -> Vec<String> {
    title
        .split(" / ")
        .map(squash)
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squash_collapses_the_archives_indentation() {
        assert_eq!(squash("  The Elfin\n   Knight  "), "The Elfin Knight");
    }

    #[test]
    fn a_dash_is_not_a_reference_number() {
        // The archive writes "Roud -" where it has no number; storing the dash
        // would poison every lookup keyed on it.
        assert!(!is_reference_value("-"));
        assert!(!is_reference_value(" — "));
        assert!(is_reference_value("12"));
        assert!(is_reference_value("P15"));
        assert!(is_reference_value("2:329"));
    }

    #[test]
    fn titles_split_on_the_separator_not_on_every_slash() {
        assert_eq!(
            split_titles("The Elfin Knight / Scarborough Fair"),
            ["The Elfin Knight", "Scarborough Fair"]
        );
        // A slash inside a title is not a separator.
        assert_eq!(split_titles("Sound/Track"), ["Sound/Track"]);
    }
}
