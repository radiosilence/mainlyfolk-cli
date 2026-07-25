//! Song-list and label parsers. Owned by the song-parser work.
//!
//! The Child index, the Laws index, the full song index and `search.php`
//! results are all the same `ul.plain > li > a` + `span.comment` markup, so
//! [`songs`] is the one parser behind all four rather than four that would
//! drift apart as the archive's hand-written HTML evolves.

use std::collections::HashSet;

use scraper::{Html, Selector};

use crate::client::resolve_path;
use crate::models::{Label, SongRefs, SongSummary, Source};

use super::{split_titles, squash};

/// A list of songs, from any of the archive's generated song lists.
pub fn songs(html: &str, base: &str) -> Vec<SongSummary> {
    let document = Html::parse_document(html);
    let li_selector = Selector::parse("ul.plain li").unwrap();
    let a_selector = Selector::parse("a").unwrap();
    let comment_selector = Selector::parse("span.comment").unwrap();

    document
        .select(&li_selector)
        .filter_map(|li| {
            let a = li.select(&a_selector).next()?;
            let href = a.attr("href").unwrap_or_default();
            let title = squash(&a.text().collect::<String>());
            let refs = li
                .select(&comment_selector)
                .next()
                .map(|c| parse_comment_refs(&c.text().collect::<String>()))
                .unwrap_or_default();

            Some(SongSummary {
                path: resolve_path(href, base),
                titles: split_titles(&title),
                title,
                refs,
                source: Source::MainlyNorfolk,
            })
        })
        .collect()
}

/// Reference numbers out of a `span.comment` like `(Roud 161; Child 1)`.
///
/// Each `;`-separated part is `Scheme Value`, sometimes several values comma
/// listed under one scheme (`Roud 161, 162`). Unrecognised schemes are
/// dropped rather than erroring — the archive adds new ones over time.
fn parse_comment_refs(comment: &str) -> SongRefs {
    let mut refs = SongRefs::default();
    let inner = squash(comment);
    let inner = inner.trim().trim_start_matches('(').trim_end_matches(')');

    for part in inner.split(';') {
        let part = part.trim();
        let Some((scheme, values)) = part.split_once(' ') else {
            continue;
        };
        let target = match scheme {
            "Roud" => &mut refs.roud,
            "Child" => &mut refs.child,
            "Laws" => &mut refs.laws,
            "G/D" => &mut refs.greig_duncan,
            "Henry" => &mut refs.henry,
            _ => continue,
        };
        target.extend(
            values
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(String::from),
        );
    }

    refs
}

/// The archive's record-label discographies, from the site navigation.
///
/// Read from the nav rather than hardcoded: a fixed list drifts as labels are
/// added, and the nav is the archive's own list.
pub fn labels(html: &str) -> Vec<Label> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("nav#mainNav a[href]").unwrap();
    let mut seen = HashSet::new();

    document
        .select(&selector)
        .filter_map(|a| {
            let href = a.attr("href")?;
            let slug = href.strip_prefix("/folk/records/")?.strip_suffix(".html")?;
            if matches!(slug, "newreleases" | "search" | "index") {
                return None;
            }
            if !seen.insert(slug.to_string()) {
                return None;
            }
            Some(Label {
                id: slug.to_string(),
                path: href.to_string(),
                name: squash(&a.text().collect::<String>()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHILD_INDEX: &str = include_str!("../../tests/fixtures/child_index.html");
    const LAWS_INDEX: &str = include_str!("../../tests/fixtures/laws_index.html");
    const SEARCH_SONGS: &str = include_str!("../../tests/fixtures/search_songs.html");
    const FOLK_INDEX: &str = include_str!("../../tests/fixtures/folk_index.html");

    #[test]
    fn child_index_yields_every_entry_the_archive_lists() {
        let entries = songs(CHILD_INDEX, "/folk/songs/childindex.html");
        // The full Child catalogue is 305 numbers, but not every one has a
        // page on the archive; this fixture's real count is 215.
        assert!(entries.len() >= 200, "got {} entries", entries.len());

        let riddles = entries
            .iter()
            .find(|s| s.refs.child == ["1"])
            .expect("Child 1 present");
        assert!(riddles.title.contains("Riddles Wisely Expounded"));
    }

    #[test]
    fn laws_index_parses_with_the_same_markup() {
        let entries = songs(LAWS_INDEX, "/folk/songs/lawsindex.html");
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|s| s.refs.laws == ["A1"]));
    }

    #[test]
    fn search_results_carry_every_reference_scheme_in_one_comment() {
        let entries = songs(SEARCH_SONGS, "/folk/songs/search.php");
        assert_eq!(entries.len(), 1);
        let song = &entries[0];
        assert_eq!(song.path, "/lloyd/songs/themountainshigh.html");
        assert_eq!(song.refs.roud, ["397"]);
        assert_eq!(song.refs.laws, ["P15"]);
        assert_eq!(song.refs.greig_duncan, ["2:333"]);
    }

    #[test]
    fn labels_come_from_the_records_nav_not_a_hardcoded_list() {
        let labels = labels(FOLK_INDEX);
        assert!(labels.len() >= 18, "got {} labels", labels.len());
        assert!(labels.iter().any(|l| l.id == "topic"));
        assert!(labels.iter().any(|l| l.id == "fellside"));
    }
}
