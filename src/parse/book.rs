//! Bibliography parser. Owned by the bibliography work.
//!
//! Song-page citations link here by anchor (`/folk/books/#<id>`), so parsing
//! the bibliography is what turns a cited title into a record with a
//! publisher, a year, and sometimes a link to the full text.

use scraper::{ElementRef, Html, Node, Selector};

use crate::client::resolve_path;
use crate::models::Book;
use crate::parse::squash;

/// The bibliography page: `article#mainArticle` holding a `<article id="intro">`
/// (a table of contents, skipped) followed by one `<article>` per section, each
/// full of `<table class="album">` entries.
pub fn index(html: &str, base: &str) -> Vec<Book> {
    let document = Html::parse_document(html);
    let section_sel = Selector::parse("article#mainArticle > article").unwrap();
    let section_title_sel = Selector::parse("p.reference > strong").unwrap();
    let table_sel = Selector::parse("table.album").unwrap();
    let cite_sel = Selector::parse("cite").unwrap();
    let strong_sel = Selector::parse("strong").unwrap();
    let a_sel = Selector::parse("a").unwrap();
    let img_sel = Selector::parse("img").unwrap();

    let mut books = Vec::new();

    for section in document.select(&section_sel) {
        if section.attr("id") == Some("intro") {
            continue;
        }
        let section_name = section
            .select(&section_title_sel)
            .next()
            .map(element_text)
            .filter(|s| !s.is_empty());

        for table in section.select(&table_sel) {
            // No id means nothing can ever link to this entry, so it's not
            // worth carrying.
            let Some(id) = table.attr("id") else {
                continue;
            };

            // Most entries title themselves with a linked `<cite>`. A handful
            // in the fixture (biographies mostly) skip `<cite>` entirely and
            // wrap the title in `<strong><a>` instead — same shape, different
            // tag, so fall back to `<strong>` when there's no `<cite>` at all.
            let cites: Vec<_> = table.select(&cite_sel).collect();
            let markers: Vec<_> = if cites.is_empty() {
                table.select(&strong_sel).collect()
            } else {
                cites
            };
            let (Some(&first), Some(&last)) = (markers.first(), markers.last()) else {
                continue;
            };

            let mut title = element_text(first);
            if let Some(stripped) = title.strip_suffix('.') {
                title = stripped.trim().to_string();
            }
            if title.is_empty() {
                continue;
            }

            let authors = split_authors(&text_before(first));
            let (publisher, year) = split_publisher_and_year(&text_after(last));

            let anchors: Vec<_> = table.select(&a_sel).collect();
            let online_url = anchors
                .iter()
                .find_map(|a| a.attr("href").filter(|h| is_absolute(h)))
                .map(String::from);
            let own_page = anchors.iter().find_map(|a| {
                a.attr("href")
                    .filter(|h| !is_absolute(h) && !h.starts_with('#'))
            });
            let path = match own_page {
                Some(href) => resolve_path(href, base),
                None => format!("{base}#{id}"),
            };
            let cover_url = table
                .select(&img_sel)
                .next()
                .and_then(|img| img.attr("src"))
                .map(|src| resolve_path(src, base));

            books.push(Book {
                id: id.to_string(),
                path,
                title,
                authors,
                publisher,
                year,
                section: section_name.clone(),
                online_url,
                cover_url,
            });
        }
    }

    books
}

fn is_absolute(href: &str) -> bool {
    href.starts_with("http://") || href.starts_with("https://")
}

/// Squashed text of an element's whole subtree. Sibling text runs are joined
/// with a space rather than concatenated raw, because the archive uses bare
/// `<br>` to separate a title from its subtitle with no whitespace either
/// side (`Songs of the West<br>Folk Songs of Devon...`).
fn element_text(el: ElementRef) -> String {
    squash(&el.text().collect::<Vec<_>>().join(" "))
}

/// The squashed text of `marker`'s preceding siblings, in document order —
/// the author line the archive writes just before a book's title.
fn text_before(marker: ElementRef) -> String {
    let mut parts: Vec<String> = marker
        .prev_siblings()
        .map(|node| match node.value() {
            Node::Text(text) => text.to_string(),
            Node::Element(_) => ElementRef::wrap(node)
                .map(|el| el.text().collect::<Vec<_>>().join(" "))
                .unwrap_or_default(),
            _ => String::new(),
        })
        .filter(|s| !s.trim().is_empty())
        .collect();
    parts.reverse();
    squash(&parts.join(" "))
}

/// The squashed text of `marker`'s following siblings, in document order —
/// the publisher/year line the archive writes just after a book's title.
fn text_after(marker: ElementRef) -> String {
    let parts: Vec<String> = marker
        .next_siblings()
        .map(|node| match node.value() {
            Node::Text(text) => text.to_string(),
            Node::Element(_) => ElementRef::wrap(node)
                .map(|el| el.text().collect::<Vec<_>>().join(" "))
                .unwrap_or_default(),
            _ => String::new(),
        })
        .filter(|s| !s.trim().is_empty())
        .collect();
    squash(&parts.join(" "))
}

/// Authors as the archive credits them: comma- and "and"-separated names,
/// with the separator that used to lead into the title trimmed off first.
fn split_authors(raw: &str) -> Vec<String> {
    let cleaned = raw.trim().trim_end_matches([',', ':']).trim();
    if cleaned.is_empty() {
        return Vec::new();
    }
    cleaned
        .split(',')
        .flat_map(|part| part.split(" and "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Publisher and year from the text after a book's title. The year is the
/// last 18xx/19xx/20xx run in the text — reprints get listed after the
/// original edition, and the most recent date is the one worth surfacing.
fn split_publisher_and_year(raw: &str) -> (Option<String>, Option<String>) {
    let publisher_text = match find_year(raw) {
        Some((year, start, end)) => {
            let mut without_year = String::with_capacity(raw.len());
            without_year.push_str(&raw[..start]);
            without_year.push_str(&raw[end..]);
            let publisher = clean_publisher(&without_year);
            return (
                if publisher.is_empty() {
                    None
                } else {
                    Some(publisher)
                },
                Some(year),
            );
        }
        None => raw,
    };
    let publisher = clean_publisher(publisher_text);
    (
        if publisher.is_empty() {
            None
        } else {
            Some(publisher)
        },
        None,
    )
}

fn clean_publisher(text: &str) -> String {
    squash(text.trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':')))
}

/// The last standalone 18xx/19xx/20xx run in `text`, with its byte range so
/// the caller can strip it back out to build the publisher string.
fn find_year(text: &str) -> Option<(String, usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = None;
    for i in 0..chars.len() {
        if i + 4 > chars.len() {
            break;
        }
        let window = &chars[i..i + 4];
        let digits: String = window.iter().map(|(_, c)| *c).collect();
        if !window.iter().all(|(_, c)| c.is_ascii_digit()) {
            continue;
        }
        let is_year_prefix = matches!(&digits[..2], "18" | "19" | "20");
        if !is_year_prefix {
            continue;
        }
        let before_ok = i == 0 || !chars[i - 1].1.is_ascii_digit();
        let after_ok = i + 4 == chars.len() || !chars[i + 4].1.is_ascii_digit();
        if before_ok && after_ok {
            let start = window[0].0;
            let end = if i + 4 < chars.len() {
                chars[i + 4].0
            } else {
                text.len()
            };
            found = Some((digits, start, end));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/books_index.html");
    const BASE: &str = "/folk/books/";

    fn books() -> Vec<Book> {
        index(FIXTURE, BASE)
    }

    #[test]
    fn finds_every_book_in_the_fixture() {
        // 178 tables carry `class="album"`; 2 have no `id` and are skipped.
        let books = books();
        assert_eq!(books.len(), 176);
    }

    #[test]
    fn ashton_is_anchor_only_with_an_external_link_and_no_own_page() {
        let books = books();
        let ashton = books
            .iter()
            .find(|b| b.id == "ashton:modernstreetballads")
            .expect("ashton entry");
        assert_eq!(ashton.title, "Modern Street Ballads");
        assert_eq!(ashton.authors, ["John Ashton"]);
        assert_eq!(ashton.year.as_deref(), Some("1888"));
        assert!(
            ashton
                .publisher
                .as_deref()
                .unwrap_or_default()
                .contains("Chatto"),
            "publisher was {:?}",
            ashton.publisher
        );
        assert!(
            ashton
                .online_url
                .as_deref()
                .unwrap_or_default()
                .contains("gutenberg.org")
        );
        assert!(ashton.path.ends_with("#ashton:modernstreetballads"));
    }

    #[test]
    fn baringgould_has_its_own_page_and_two_authors() {
        let books = books();
        let baringgould = books
            .iter()
            .find(|b| b.id == "baringgould:songsofthewest")
            .expect("baringgould entry");
        assert_eq!(baringgould.authors.len(), 2);
        assert_eq!(baringgould.path, "/folk/books/songsofthewest.html");
    }

    #[test]
    fn every_book_has_an_id_and_a_title() {
        for book in books() {
            assert!(!book.id.is_empty(), "book with empty id: {book:?}");
            assert!(
                !book.title.is_empty(),
                "book {} has an empty title",
                book.id
            );
        }
    }

    #[test]
    fn every_path_is_site_absolute() {
        for book in books() {
            assert!(
                book.path.starts_with('/'),
                "book {} has a non-absolute path: {}",
                book.id,
                book.path
            );
        }
    }

    #[test]
    fn at_least_two_sections_are_represented() {
        let sections: std::collections::HashSet<_> =
            books().into_iter().filter_map(|b| b.section).collect();
        assert!(sections.len() >= 2, "sections were: {sections:?}");
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = books().into_iter().map(|b| b.id).collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate book ids found");
    }

    #[test]
    fn a_title_wrapped_in_strong_instead_of_cite_is_still_found() {
        // The archive's biographies section titles some entries with
        // `<strong><a>` rather than `<cite>` — the fallback this exercises.
        let books = books();
        let copper = books
            .iter()
            .find(|b| b.id == "copper:asongforeveryseason")
            .expect("copper entry");
        assert_eq!(copper.title, "A Song for Every Season");
        assert_eq!(copper.authors, ["Bob Copper"]);
    }
}
