//! Album page parser. Owned by the artist-parser work.
//!
//! The one fixture available (`album_martincarthy.html`) documents a single
//! release inside an `<article id="..." data-performer="..." data-year="...">`
//! wrapper with a table of catalogue lines per pressing — not the comma-tail
//! `<p>` shape [`crate::parse::artist::discography`] parses. Both shapes are
//! handled: the data attributes and table line for pages like the fixture, the
//! `<cite>`-bearing `<p>` shape (reusing `artist`'s own helpers) for a
//! multi-release page reached by `#fragment` into an `<h2 id="...">` section, in
//! case one ever looks like a discography entry instead.
use scraper::{ElementRef, Html, Selector};

use crate::error::{Error, Result};
use crate::models::{Album, Source, Track};
use crate::parse::artist::{
    find_format, parse_release_tail, split_around_cites, split_label_and_catalogue,
};
use crate::parse::squash;

pub fn parse(html: &str, path: &str) -> Result<(Album, Vec<Track>)> {
    let doc = Html::parse_document(html);
    let has_heading = doc
        .select(&Selector::parse("h1, h2").unwrap())
        .next()
        .is_some();
    if !has_heading {
        return Err(Error::Parse {
            what: "album",
            url: path.into(),
        });
    }

    let fragment = path.split_once('#').map(|(_, f)| f);
    let main_sel = Selector::parse("article#mainArticle").unwrap();
    let main = doc.select(&main_sel).next();

    let mut marker_heading: Option<String> = None;
    let scope: Vec<ElementRef> = match fragment.and_then(|f| find_by_id(&doc, f)) {
        Some(marker) if is_heading(marker) => {
            marker_heading = Some(squash(&marker.text().collect::<String>()));
            heading_section(marker)
        }
        Some(marker) => vec![marker],
        None => main.into_iter().collect(),
    };

    let title = marker_heading
        .or_else(|| first_text_in(&scope, "h1"))
        .or_else(|| first_text_in(&scope, "h2"))
        .or_else(|| {
            doc.select(&Selector::parse("h1").unwrap())
                .next()
                .map(|e| squash(&e.text().collect::<String>()))
        })
        .unwrap_or_default();

    let performer = first_attr(&scope, "data-performer");
    let data_year = first_attr(&scope, "data-year");

    let (table_label, table_catalogue, table_format) = catalogue_from_table(&scope);
    let (cite_artist, cite_meta) = catalogue_from_cite_paragraph(&scope);

    let artist = performer.or(cite_artist);
    let year = data_year.or(cite_meta.as_ref().and_then(|m| m.year.clone()));
    let label = table_label.or_else(|| cite_meta.as_ref().and_then(|m| m.label.clone()));
    let catalogue_number =
        table_catalogue.or_else(|| cite_meta.as_ref().and_then(|m| m.catalogue_number.clone()));
    let format = table_format.or_else(|| cite_meta.as_ref().and_then(|m| m.format.clone()));

    let cover_url = first_img_src(&scope, path);
    let tracks = parse_tracks(&scope, path);

    Ok((
        Album {
            path: path.to_string(),
            title,
            artist,
            year,
            label,
            catalogue_number,
            format,
            cover_url,
            source: Source::MainlyNorfolk,
        },
        tracks,
    ))
}

fn find_by_id<'a>(doc: &'a Html, id: &str) -> Option<ElementRef<'a>> {
    let selector = Selector::parse(&format!("[id=\"{id}\"]")).ok()?;
    doc.select(&selector).next()
}

fn is_heading(el: ElementRef) -> bool {
    matches!(el.value().name(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// `marker` plus its following siblings, stopping before the next sibling that
/// shares its tag name — the section a `#fragment` into an `<h2 id="...">`
/// introduces.
fn heading_section(marker: ElementRef) -> Vec<ElementRef> {
    let tag = marker.value().name().to_string();
    let mut section = vec![marker];
    for node in marker.next_siblings() {
        let Some(el) = ElementRef::wrap(node) else {
            continue;
        };
        if el.value().name() == tag {
            break;
        }
        section.push(el);
    }
    section
}

fn first_text_in(scope: &[ElementRef], tag: &str) -> Option<String> {
    let selector = Selector::parse(tag).ok()?;
    scope
        .iter()
        .find_map(|el| el.select(&selector).next())
        .map(|el| squash(&el.text().collect::<String>()))
}

/// Checks each scope element's own attribute before its descendants': a
/// `#fragment` landing on a container like `<article data-performer="...">`
/// carries the attribute itself, not on a child.
fn first_attr(scope: &[ElementRef], attr: &str) -> Option<String> {
    for el in scope {
        if let Some(v) = el.value().attr(attr) {
            return Some(v.to_string());
        }
    }
    let selector = Selector::parse(&format!("[{attr}]")).ok()?;
    scope
        .iter()
        .find_map(|el| el.select(&selector).next())
        .and_then(|el| el.value().attr(attr))
        .map(String::from)
}

fn first_img_src(scope: &[ElementRef], base: &str) -> Option<String> {
    let selector = Selector::parse("img").unwrap();
    scope
        .iter()
        .find_map(|el| el.select(&selector).next())
        .and_then(|img| img.value().attr("src"))
        .map(|src| crate::client::resolve_path(src, base))
}

/// The first catalogue line inside a `table.album` cell, e.g.
/// `"Fontana TL 5269 (mono LP, UK, 1965)"` — one line per pressing, so only the
/// first (the release's primary pressing) is used. Distinguished from the
/// title/performer `<p>` that precedes it in the same cell by having a `(`.
fn catalogue_from_table(scope: &[ElementRef]) -> (Option<String>, Option<String>, Option<String>) {
    let table_p_sel = Selector::parse("table.album p").unwrap();
    let Some(p) = scope.iter().find_map(|el| {
        el.select(&table_p_sel)
            .find(|p| p.text().any(|t| t.contains('(')))
    }) else {
        return (None, None, None);
    };

    let first_line = p
        .children()
        .find_map(|n| n.value().as_text().map(|t| t.text.trim().to_string()))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| p.text().collect::<String>());
    let line = squash(&first_line);

    let (head, paren) = match line.split_once('(') {
        Some((h, rest)) => (h.trim(), rest.trim_end_matches(')').trim()),
        None => (line.as_str(), ""),
    };
    let (label, catalogue_number) = split_label_and_catalogue(head);
    let format = paren
        .split(',')
        .find_map(|token| find_format(token.trim()))
        .map(String::from);
    (label, catalogue_number, format)
}

/// The artist and tail metadata from the first `<cite>`-bearing `<p>` in scope,
/// reusing exactly the shape `artist::discography` parses — for a multi-release
/// album page whose `#fragment` section is written the same way as a
/// discography entry rather than as this fixture's table.
fn catalogue_from_cite_paragraph(
    scope: &[ElementRef],
) -> (Option<String>, Option<crate::parse::artist::ReleaseMeta>) {
    let p_sel = Selector::parse("p").unwrap();
    let cite_sel = Selector::parse("cite").unwrap();
    let Some(p) = scope.iter().find_map(|el| {
        el.select(&p_sel)
            .find(|p| p.select(&cite_sel).next().is_some())
    }) else {
        return (None, None);
    };
    let (before, tail) = split_around_cites(p);
    let artist = {
        let t = before.trim_end_matches(':').trim();
        (!t.is_empty()).then(|| t.to_string())
    };
    (artist, Some(parse_release_tail(&tail, None)))
}

/// Every `<ol>`/`<ul>` in scope, in document order — the fixture splits a
/// tracklist across several `<ol start="N">` (one per LP side), so all of them
/// are collected rather than just the first; positions honour each list's own
/// `start` so side two continues the numbering rather than restarting at 1.
fn parse_tracks(scope: &[ElementRef], base: &str) -> Vec<Track> {
    let list_sel = Selector::parse("ol, ul").unwrap();
    let li_sel = Selector::parse("li").unwrap();
    let song_link_sel = Selector::parse("a[href]").unwrap();

    let mut tracks = Vec::new();
    for el in scope {
        for list in el.select(&list_sel) {
            let ordered = list.value().name() == "ol";
            let start: u32 = list
                .value()
                .attr("start")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);

            for (i, li) in list.select(&li_sel).enumerate() {
                let position = ordered.then(|| start + i as u32);
                let raw = squash(&li.text().collect::<String>());
                let (title, duration) = split_trailing_duration(&raw);

                let song_path = li
                    .select(&song_link_sel)
                    .find(|a| {
                        a.value()
                            .attr("href")
                            .is_some_and(|h| h.contains("/songs/"))
                    })
                    .or_else(|| li.select(&song_link_sel).next())
                    .and_then(|a| a.value().attr("href"))
                    .map(|href| crate::client::resolve_path(href, base));

                tracks.push(Track {
                    position,
                    title,
                    song_path,
                    duration,
                });
            }
        }
    }
    tracks
}

/// Splits a trailing `"(m:ss)"` or `"(m.ss)"` off squashed `<li>` text — the
/// fixture writes track times as `"(2.31)"`, a decimal point rather than the
/// colon [`Track::duration`]'s own doc comment shows, so both are accepted and
/// kept exactly as printed.
fn split_trailing_duration(text: &str) -> (String, Option<String>) {
    let Some(rest) = text.strip_suffix(')') else {
        return (text.to_string(), None);
    };
    let Some(open) = rest.rfind('(') else {
        return (text.to_string(), None);
    };
    let inner = &rest[open + 1..];
    let Some((mins, secs)) = inner.split_once(['.', ':']) else {
        return (text.to_string(), None);
    };
    if !mins.is_empty()
        && mins.len() <= 3
        && secs.len() == 2
        && mins.bytes().all(|b| b.is_ascii_digit())
        && secs.bytes().all(|b| b.is_ascii_digit())
    {
        let title = text[..open].trim_end().to_string();
        (title, Some(inner.to_string()))
    } else {
        (text.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALBUM_MARTINCARTHY: &str = include_str!("../../tests/fixtures/album_martincarthy.html");

    #[test]
    fn album_page_gives_title_and_tracks() {
        let (album, tracks) = parse(
            ALBUM_MARTINCARTHY,
            "/martin.carthy/records/martincarthy.html",
        )
        .unwrap();
        assert!(!album.title.is_empty());
        assert_eq!(album.title, "Martin Carthy");
        assert_eq!(album.artist.as_deref(), Some("Martin Carthy"));
        assert_eq!(album.year.as_deref(), Some("1965"));
        // Reported in the PR: how many tracks this fixture's two `<ol>` sides give.
        assert_eq!(tracks.len(), 14);
        assert!(tracks.iter().any(|t| t.song_path.is_some()));
        assert!(tracks.iter().any(|t| t.duration.is_some()));
    }

    #[test]
    fn a_page_with_no_heading_at_all_is_a_parse_error() {
        let err = parse(
            "<html><body><p>no headings here</p></body></html>",
            "/x.html",
        )
        .unwrap_err();
        assert!(matches!(err, Error::Parse { what: "album", .. }));
    }

    #[test]
    fn an_album_with_no_tracklist_returns_no_tracks_not_an_error() {
        let (album, tracks) =
            parse("<html><body><h1>A Bare Page</h1></body></html>", "/x.html").unwrap();
        assert_eq!(album.title, "A Bare Page");
        assert!(tracks.is_empty());
    }
}
