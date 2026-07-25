//! Album page parser. Owned by the artist-parser work.
//!
//! `album_martincarthy.html` documents its release as a self-contained
//! `<article id="..." data-title="..." data-performer="..." data-year="...">`:
//! structured data for title/artist/year, a `table.album` of per-pressing
//! catalogue lines, and a `<h2>Tracks</h2>` section split across one `<ol>` per
//! LP side. The data attributes are authoritative and preferred over anything
//! scraped from text; a `<cite>`-bearing `<p>` (the shape
//! [`crate::parse::artist::discography`] parses) is tried only as a fallback,
//! for a hypothetical page shaped like a discography entry instead.
use scraper::{CaseSensitivity, ElementRef, Html, Node, Selector};

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
    let scope: Vec<ElementRef> = match fragment {
        // `#fragment` always names one of the page's own release articles.
        Some(frag) => match find_release_article(&doc, frag).or_else(|| find_by_id(&doc, frag)) {
            Some(marker) if is_heading(marker) => {
                marker_heading = Some(squash(&marker.text().collect::<String>()));
                heading_section(marker)
            }
            Some(marker) => vec![marker],
            None => main.into_iter().collect(),
        },
        // No fragment: the page's first release article, or the whole thing if
        // it isn't structured that way.
        None => {
            let release_sel = Selector::parse("article[data-title]").unwrap();
            main.and_then(|m| m.select(&release_sel).next())
                .or(main)
                .into_iter()
                .collect()
        }
    };

    let title = marker_heading
        .or_else(|| first_attr(&scope, "data-title"))
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
    // Only worth trying when the structured data didn't already answer it.
    let (cite_artist, cite_meta) = if performer.is_none() || data_year.is_none() {
        catalogue_from_cite_paragraph(&scope)
    } else {
        (None, None)
    };

    let artist = performer.or(cite_artist);
    let year = data_year.or_else(|| cite_meta.as_ref().and_then(|m| m.year.clone()));
    let label = table_label.or_else(|| cite_meta.as_ref().and_then(|m| m.label.clone()));
    let catalogue_number =
        table_catalogue.or_else(|| cite_meta.as_ref().and_then(|m| m.catalogue_number.clone()));
    let format = table_format.or_else(|| cite_meta.as_ref().and_then(|m| m.format.clone()));

    let cover_url = first_table_img_src(&scope, path);
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

fn find_release_article<'a>(doc: &'a Html, id: &str) -> Option<ElementRef<'a>> {
    let selector = Selector::parse(&format!("article[id=\"{id}\"]")).ok()?;
    doc.select(&selector).next()
}

fn is_heading(el: ElementRef) -> bool {
    matches!(el.value().name(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// `marker` plus its following siblings, stopping before the next sibling that
/// shares its tag name. Kept as a fallback for a `#fragment` landing on a
/// heading rather than a release `<article>`.
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

/// Checks each scope element's own attribute before its descendants': the
/// release `<article data-performer="...">` carries the attribute itself, not
/// on a child.
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

fn first_table_img_src(scope: &[ElementRef], base: &str) -> Option<String> {
    let table_sel = Selector::parse("table.album img").unwrap();
    let any_sel = Selector::parse("img").unwrap();
    scope
        .iter()
        .find_map(|el| el.select(&table_sel).next())
        .or_else(|| scope.iter().find_map(|el| el.select(&any_sel).next()))
        .and_then(|img| img.value().attr("src"))
        .map(|src| crate::client::resolve_path(src, base))
}

/// The first catalogue line inside a `table.album` cell, e.g.
/// `"Fontana TL 5269 (mono LP, UK, 1965)"` — one line per pressing (`<br>`
/// separated, sometimes with the label as a link), so only the first (the
/// release's primary pressing) is used. Distinguished from the title/performer
/// `<p>` that precedes it in the same cell by having a `(`.
fn catalogue_from_table(scope: &[ElementRef]) -> (Option<String>, Option<String>, Option<String>) {
    let table_p_sel = Selector::parse("table.album p").unwrap();
    let Some(p) = scope.iter().find_map(|el| {
        el.select(&table_p_sel)
            .find(|p| p.text().any(|t| t.contains('(')))
    }) else {
        return (None, None, None);
    };

    let line = squash(&text_before_first_br(p));
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

/// Text of `el`'s children up to (excluding) its first `<br>`, with inline
/// elements like an `<a class="external">` label link flattened to their text.
fn text_before_first_br(el: ElementRef) -> String {
    let mut out = String::new();
    for node in el.children() {
        if node.value().as_element().is_some_and(|e| e.name() == "br") {
            break;
        }
        match node.value() {
            Node::Text(t) => out.push_str(&t.text),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(node) {
                    out.push_str(&child.text().collect::<String>());
                }
            }
            _ => {}
        }
    }
    out
}

/// The artist and tail metadata from the first `<cite>`-bearing `<p>` in scope,
/// reusing exactly the shape `artist::discography` parses — for a release page
/// with no `data-performer`/`data-year` of its own, in case one is ever shaped
/// like a discography entry rather than this fixture's table.
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

/// Every `<ol>`/`<ul>` in scope, in document order — an album is split across
/// sides and each side is its own `<ol>`, so all of them are collected rather
/// than just the first. Position honours each list's own `start` attribute
/// (default 1) so a later side continues the numbering instead of restarting.
fn parse_tracks(scope: &[ElementRef], base: &str) -> Vec<Track> {
    let list_sel = Selector::parse("ol, ul").unwrap();
    let li_sel = Selector::parse("li").unwrap();
    let link_sel = Selector::parse("a[href]").unwrap();
    let time_sel = Selector::parse("span.time").unwrap();

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

                let first_link = li.select(&link_sel).next();
                let title = match first_link {
                    Some(a) => squash(&a.text().collect::<String>()),
                    None => squash(&track_text_excluding_annotations(li)),
                };
                let song_path = first_link
                    .and_then(|a| a.value().attr("href"))
                    .map(|href| crate::client::resolve_path(href, base));
                let duration = li
                    .select(&time_sel)
                    .next()
                    .map(|s| squash(&s.text().collect::<String>()))
                    .map(|s| s.trim_matches(|c| c == '(' || c == ')').to_string());

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

/// A track `<li>`'s text with its `span.comment` (Roud/Child refs) and
/// `span.time` (duration) subtrees removed — the fallback when a track has no
/// `<a>` to take the title from directly.
fn track_text_excluding_annotations(li: ElementRef) -> String {
    let mut out = String::new();
    for node in li.children() {
        if let Some(el) = node.value().as_element()
            && el.name() == "span"
            && (el.has_class("comment", CaseSensitivity::CaseSensitive)
                || el.has_class("time", CaseSensitivity::CaseSensitive))
        {
            continue;
        }
        match node.value() {
            Node::Text(t) => out.push_str(&t.text),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(node) {
                    out.push_str(&child.text().collect::<String>());
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALBUM_MARTINCARTHY: &str = include_str!("../../tests/fixtures/album_martincarthy.html");

    #[test]
    fn album_page_gives_title_artist_year_and_label_from_structured_data() {
        let (album, tracks) = parse(
            ALBUM_MARTINCARTHY,
            "/martin.carthy/records/martincarthy.html",
        )
        .unwrap();
        assert_eq!(album.title, "Martin Carthy");
        assert_eq!(album.artist.as_deref(), Some("Martin Carthy"));
        assert_eq!(album.year.as_deref(), Some("1965"));
        assert!(album.label.as_deref().unwrap().contains("Fontana"));
        assert_eq!(tracks.len(), 14);
    }

    #[test]
    fn tracks_carry_titles_durations_and_song_paths_across_both_sides() {
        let (_, tracks) = parse(
            ALBUM_MARTINCARTHY,
            "/martin.carthy/records/martincarthy.html",
        )
        .unwrap();

        let first = &tracks[0];
        assert_eq!(first.position, Some(1));
        assert_eq!(first.title, "High Germany");
        assert_eq!(first.duration.as_deref(), Some("2.31"));
        assert!(
            first
                .song_path
                .as_deref()
                .unwrap()
                .ends_with("highgermany.html")
        );

        // Side 2's `<ol start="8">` continues the numbering rather than
        // restarting — position 8 is track index 7, not a second track 1.
        let eighth = tracks.iter().find(|t| t.position == Some(8)).unwrap();
        assert_eq!(eighth.title, "Scarborough Fair");
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
