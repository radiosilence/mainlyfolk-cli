//! Search, a single song page, and the Child/Laws ballad indexes.

use crate::archive::{RefKey, SongQuery};
use crate::error::Result;
use crate::models::{Output, SongSummary};

use super::archive;

pub async fn search(
    no_cache: bool,
    query: Option<&str>,
    author: Option<&str>,
    scheme: Option<RefKey>,
    number: Option<&str>,
) -> Result<()> {
    let archive = archive(no_cache)?;
    let query = build_song_query(query, author, scheme, number);
    Output::success(archive.search_songs(&query).await?).print();
    Ok(())
}

/// Split out from [`search`] so the argument-to-query mapping is testable
/// without a network call — `Archive::search_songs` is where the request
/// actually happens.
fn build_song_query(
    title: Option<&str>,
    author: Option<&str>,
    key: Option<RefKey>,
    keyval: Option<&str>,
) -> SongQuery {
    SongQuery {
        title: title.map(String::from),
        author: author.map(String::from),
        key,
        keyval: keyval.map(String::from),
    }
}

pub async fn song(no_cache: bool, path: &str, lyrics_only: bool) -> Result<()> {
    let archive = archive(no_cache)?;
    let song = archive.song(path).await?;
    if lyrics_only {
        Output::success(song.lyrics).print();
    } else {
        Output::success(song).print();
    }
    Ok(())
}

pub async fn child(no_cache: bool, filter: Option<&str>) -> Result<()> {
    let archive = archive(no_cache)?;
    let index = archive.child_index().await?;
    Output::success(filter_index(index, filter, |s| s.refs.child.as_slice())).print();
    Ok(())
}

pub async fn laws(no_cache: bool, filter: Option<&str>) -> Result<()> {
    let archive = archive(no_cache)?;
    let index = archive.laws_index().await?;
    Output::success(filter_index(index, filter, |s| s.refs.laws.as_slice())).print();
    Ok(())
}

/// Child and Laws browsing share everything except which [`SongRefs`](crate::models::SongRefs)
/// field the filter reads, so that's the one thing the caller supplies.
fn filter_index(
    index: Vec<SongSummary>,
    filter: Option<&str>,
    codes_of: impl Fn(&SongSummary) -> &[String],
) -> Vec<SongSummary> {
    let Some(filter) = filter else {
        return index;
    };
    index
        .into_iter()
        .filter(|s| matches_filter(codes_of(s), &s.titles, filter))
        .collect()
}

/// A Child/Laws filter is one of, tried in this order:
/// - a range (`1-50`), both ends numeric — inclusive, against each code's
///   numeric part;
/// - a bare number (`84`) — exact match against the numeric part, so it
///   doesn't also catch `840`;
/// - a full reference code (`P15`, `84A`) — exact, case-insensitive match
///   against the whole code. Digits alone are handled by the branch above;
///   this is for a code that also carries letters, either a Laws-style
///   letter prefix or a Child-style letter suffix. Tried before the prefix
///   branch below and returning either way, so a code nothing carries stays
///   an empty list rather than falling through into a free-text match that
///   happens to hit something else;
/// - a letter prefix (`P`, occasionally two letters like `dA`) — every code
///   starting with it, case-insensitive. Capped at two characters so a real
///   word (`"scarborough"` is all-alphabetic too) falls through to free text
///   instead of being read as a prefix nothing will ever start with;
/// - anything else — free text against the title and every alternate title.
fn matches_filter(codes: &[String], titles: &[String], filter: &str) -> bool {
    if let Some((lo, hi)) = parse_range(filter) {
        return codes
            .iter()
            .filter_map(|c| numeric_part(c))
            .filter_map(|n| n.parse::<u32>().ok())
            .any(|n| (lo..=hi).contains(&n));
    }

    if !filter.is_empty() && filter.chars().all(|c| c.is_ascii_digit()) {
        return codes.iter().any(|c| numeric_part(c) == Some(filter));
    }

    if is_reference_code(filter) {
        return codes.iter().any(|c| c.eq_ignore_ascii_case(filter));
    }

    if (1..=2).contains(&filter.len()) && filter.chars().all(|c| c.is_ascii_alphabetic()) {
        let filter = filter.to_ascii_uppercase();
        return codes
            .iter()
            .any(|c| c.to_ascii_uppercase().starts_with(&filter));
    }

    let needle = filter.to_ascii_lowercase();
    titles
        .iter()
        .any(|t| t.to_ascii_lowercase().contains(&needle))
}

/// A filter shaped like a reference code rather than a search word: letters
/// and digits only, with at least one of each. Bare digits (`84`) and bare
/// letters (`P`, free text) are handled elsewhere — this is for the mixed
/// forms, `P15` (letter prefix, Laws) and `84A` (letter suffix, Child).
fn is_reference_code(filter: &str) -> bool {
    filter.chars().any(|c| c.is_ascii_digit())
        && filter.chars().any(|c| c.is_ascii_alphabetic())
        && filter.chars().all(|c| c.is_ascii_alphanumeric())
}

fn parse_range(filter: &str) -> Option<(u32, u32)> {
    let (lo, hi) = filter.split_once('-')?;
    Some((lo.trim().parse().ok()?, hi.trim().parse().ok()?))
}

/// The digits in a reference code — all of them for a Child number (`"84"`),
/// the part after the letter for a Laws code (`"P15"` -> `"15"`).
fn numeric_part(code: &str) -> Option<&str> {
    let start = code.find(|c: char| c.is_ascii_digit())?;
    Some(&code[start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SongRefs, Source};

    fn summary(titles: &[&str], child: &[&str], laws: &[&str]) -> SongSummary {
        SongSummary {
            path: "/x".into(),
            title: titles.join(" / "),
            titles: titles.iter().map(|s| s.to_string()).collect(),
            refs: SongRefs {
                child: child.iter().map(|s| s.to_string()).collect(),
                laws: laws.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            source: Source::MainlyNorfolk,
        }
    }

    #[test]
    fn build_song_query_maps_every_argument() {
        let q = build_song_query(
            Some("reynardine"),
            Some("carthy"),
            Some(RefKey::Child),
            Some("2"),
        );
        assert_eq!(q.title.as_deref(), Some("reynardine"));
        assert_eq!(q.author.as_deref(), Some("carthy"));
        assert_eq!(q.key, Some(RefKey::Child));
        assert_eq!(q.keyval.as_deref(), Some("2"));
    }

    #[test]
    fn build_song_query_with_only_a_title() {
        let q = build_song_query(Some("reynardine"), None, None, None);
        assert_eq!(q.title.as_deref(), Some("reynardine"));
        assert!(q.author.is_none() && q.key.is_none() && q.keyval.is_none());
    }

    #[test]
    fn build_song_query_with_nothing_is_empty() {
        assert!(build_song_query(None, None, None, None).is_empty());
    }

    #[test]
    fn a_bare_number_matches_the_exact_child_ballad_only() {
        let s84 = summary(&["Ballad 84"], &["84"], &[]);
        let s8 = summary(&["Ballad 8"], &["8"], &[]);
        let s840 = summary(&["Ballad 840"], &["840"], &[]);
        assert!(matches_filter(&s84.refs.child, &s84.titles, "84"));
        assert!(!matches_filter(&s8.refs.child, &s8.titles, "84"));
        assert!(!matches_filter(&s840.refs.child, &s840.titles, "84"));
    }

    #[test]
    fn a_range_is_inclusive_at_both_ends() {
        let matches_at = |n: &str| {
            let s = summary(&["x"], &[n], &[]);
            matches_filter(&s.refs.child, &s.titles, "1-50")
        };
        assert!(matches_at("1"));
        assert!(matches_at("50"));
        assert!(!matches_at("51"));
    }

    #[test]
    fn a_letter_prefix_matches_laws_codes_under_it() {
        let p15 = summary(&["x"], &[], &["P15"]);
        let q1 = summary(&["x"], &[], &["Q1"]);
        assert!(matches_filter(&p15.refs.laws, &p15.titles, "P"));
        assert!(!matches_filter(&q1.refs.laws, &q1.titles, "P"));
    }

    #[test]
    fn a_full_laws_code_matches_the_exact_entry_not_the_whole_prefix_set() {
        let p15 = summary(
            &["The Mountains High / Upon the Mountains High / Reynardine"],
            &[],
            &["P15"],
        );
        let p16 = summary(&["Some Other Ballad"], &[], &["P16"]);
        assert!(matches_filter(&p15.refs.laws, &p15.titles, "P15"));
        assert!(!matches_filter(&p16.refs.laws, &p16.titles, "P15"));
    }

    #[test]
    fn a_letter_prefix_still_returns_the_whole_set_not_just_one_code() {
        let p15 = summary(&["x"], &[], &["P15"]);
        let p16 = summary(&["y"], &[], &["P16"]);
        assert!(matches_filter(&p15.refs.laws, &p15.titles, "P"));
        assert!(matches_filter(&p16.refs.laws, &p16.titles, "P"));
    }

    #[test]
    fn a_full_reference_code_matches_regardless_of_case() {
        let p15 = summary(&["x"], &[], &["P15"]);
        assert!(matches_filter(&p15.refs.laws, &p15.titles, "p15"));
    }

    #[test]
    fn a_child_number_with_a_letter_suffix_matches_exactly() {
        let s84a = summary(&["x"], &["84A"], &[]);
        let s84b = summary(&["y"], &["84B"], &[]);
        assert!(matches_filter(&s84a.refs.child, &s84a.titles, "84A"));
        assert!(!matches_filter(&s84b.refs.child, &s84b.titles, "84A"));
    }

    #[test]
    fn a_reference_code_matching_no_entry_stays_empty_rather_than_falling_back_to_text() {
        // "P99" happens not to appear in either the codes or the title below —
        // if the exact-code branch didn't return unconditionally, this would
        // wrongly fall through to a free-text scan and could hit something.
        let s = summary(&["Ballad P99 Blues"], &[], &["P15"]);
        assert!(!matches_filter(&s.refs.laws, &s.titles, "P99"));
    }

    #[test]
    fn free_text_matches_an_alternate_title_not_just_the_first() {
        let s = summary(&["The Elfin Knight", "Scarborough Fair"], &[], &[]);
        assert!(matches_filter(&s.refs.child, &s.titles, "scarborough"));
    }

    #[test]
    fn a_filter_matching_nothing_yields_an_empty_list_not_an_error() {
        let index = vec![summary(&["Ballad 84"], &["84"], &[])];
        assert!(filter_index(index, Some("nonexistent"), |s| s.refs.child.as_slice()).is_empty());
    }
}
