//! Generic page parser. Owned by the artist-parser work.
use std::collections::HashSet;

use scraper::{ElementRef, Html, Selector};

use crate::client::resolve_path;
use crate::error::{Error, Result};
use crate::models::{Link, Page, Source};
use crate::parse::squash;

pub fn parse(html: &str, path: &str, source: Source) -> Result<Page> {
    let doc = Html::parse_document(html);

    // The page's own `<h1>`, not the site banner's — see `artist::parse`.
    let h1_sel = Selector::parse("article#mainArticle h1").unwrap();
    let fallback_h1_sel = Selector::parse("h1").unwrap();
    let title_sel = Selector::parse("title").unwrap();

    let h1 = doc
        .select(&h1_sel)
        .next()
        .or_else(|| doc.select(&fallback_h1_sel).next());
    let title_tag = doc.select(&title_sel).next();
    if h1.is_none() && title_tag.is_none() {
        return Err(Error::Parse {
            what: "page",
            url: path.into(),
        });
    }
    let title = match h1 {
        Some(el) => squash(&el.text().collect::<String>()),
        None => squash(&title_tag.unwrap().text().collect::<String>()),
    };

    let main_sel = Selector::parse("article#mainArticle").unwrap();
    let body_sel = Selector::parse("body").unwrap();
    let scope = doc
        .select(&main_sel)
        .next()
        .or_else(|| doc.select(&body_sel).next());

    let mut text = String::new();
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    if let Some(scope) = scope {
        let content_sel = Selector::parse("p, li, h2, h3, blockquote, pre").unwrap();
        for el in scope.select(&content_sel) {
            if in_excluded_subtree(el) {
                continue;
            }
            let squashed = squash(&el.text().collect::<String>());
            if squashed.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            match el.value().name() {
                "h2" | "h3" => {
                    text.push_str("## ");
                    text.push_str(&squashed);
                }
                _ => text.push_str(&squashed),
            }
        }

        let link_sel = Selector::parse("a[href]").unwrap();
        for a in scope.select(&link_sel) {
            if in_excluded_subtree(a) {
                continue;
            }
            let link_text = squash(&a.text().collect::<String>());
            if link_text.is_empty() {
                continue;
            }
            let url = resolve_path(a.value().attr("href").unwrap(), path);
            if seen.insert(url.clone()) {
                links.push(Link {
                    text: link_text,
                    url,
                });
            }
        }
    }

    Ok(Page {
        path: path.to_string(),
        title,
        text,
        links,
        source,
    })
}

/// Whether `el` sits under `nav#mainNav` or a `<footer>` — belt and braces for
/// when scope has fallen back to `<body>` (no `#mainArticle` on the page) and
/// those subtrees are siblings-turned-descendants instead of excluded by scope
/// alone.
fn in_excluded_subtree(el: ElementRef) -> bool {
    el.ancestors().any(|node| {
        ElementRef::wrap(node).is_some_and(|e| {
            e.value().name() == "footer"
                || (e.value().name() == "nav" && e.value().id() == Some("mainNav"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST_CHANGES: &str = include_str!("../../tests/fixtures/latest_changes.html");
    const ARTIST_INDEX: &str = include_str!("../../tests/fixtures/artist_index.html");

    #[test]
    fn latest_changes_gives_title_text_and_links_without_nav() {
        let page = parse(
            LATEST_CHANGES,
            "/folk/latestchanges.html",
            Source::MainlyNorfolk,
        )
        .unwrap();
        assert_eq!(page.title, "Latest Changes");
        assert!(page.text.len() > 500, "got {} chars", page.text.len());
        assert!(!page.links.is_empty());
        assert!(
            !page.links.iter().any(|l| l.text == "Fellside Discography"),
            "nav content leaked into links"
        );
    }

    #[test]
    fn any_fixture_with_a_mainarticle_parses_too() {
        let page = parse(ARTIST_INDEX, "/martin.carthy/", Source::MainlyNorfolk).unwrap();
        assert!(page.title.contains("Martin Carthy"));
        assert!(!page.links.iter().any(|l| l.text == "Fellside Discography"));
    }

    #[test]
    fn a_document_with_neither_h1_nor_title_is_a_parse_error() {
        let err = parse(
            "<html><body><p>nothing</p></body></html>",
            "/x.html",
            Source::MainlyNorfolk,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Parse { what: "page", .. }));
    }
}
