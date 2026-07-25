//! Song page parser. Owned by the song-parser work; see `parse::song`.
//!
//! A song page is one `<article id="mainArticle">`: a breadcrumb naming every
//! artist that claims the page, a bracketed reference line, an optional
//! bibliography, then any number of `<h3>` lyrics blocks, blockquoted notes
//! and prose recording citations, all as siblings rather than nested
//! sections. Each block below is found and read independently because the
//! archive doesn't group them any more tightly than that.

use std::collections::HashSet;

use scraper::{CaseSensitivity, ElementRef, Html, Node, Selector};

use crate::client::resolve_path;
use crate::error::{Error, Result};
use crate::models::{
    Attribution, BookRef, Link, LyricVersion, Note, Recording, Song, SongRefs, Source,
};

use super::{split_titles, squash};

pub fn parse(html: &str, path: &str) -> Result<Song> {
    let document = Html::parse_document(html);
    let article_selector = Selector::parse("article#mainArticle").unwrap();
    let h1_selector = Selector::parse("h1").unwrap();

    let article = document.select(&article_selector).next();
    let title_el = article.as_ref().and_then(|a| a.select(&h1_selector).next());
    let (Some(article), Some(title_el)) = (article, title_el) else {
        return Err(Error::Parse {
            what: "song",
            url: path.into(),
        });
    };

    let title = squash(&title_el.text().collect::<String>());
    let (refs, external_links, traditional) = parse_refs(&article);

    Ok(Song {
        path: path.to_string(),
        titles: split_titles(&title),
        title,
        refs,
        attributions: parse_attributions(&article, path),
        lyrics: parse_lyrics(&article),
        notes: parse_notes(&article),
        recordings: parse_recordings(&article, path),
        bibliography: parse_bibliography(&article, path),
        external_links,
        traditional,
        source: Source::MainlyNorfolk,
    })
}

/// Each breadcrumb line in `p.reference` is one artist's claim on the page:
/// `> Artist > Songs > **Their Title**`, lines separated by `<br>`. The first
/// crumb on a page with no artist section reads `Folk Music`, which isn't an
/// attribution and is skipped.
fn parse_attributions(article: &ElementRef, base: &str) -> Vec<Attribution> {
    let selector = Selector::parse("p.reference").unwrap();
    let Some(reference) = article.select(&selector).next() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut first_link: Option<(String, String)> = None;
    let mut strong_text: Option<String> = None;

    for child in reference.children() {
        let Some(el) = ElementRef::wrap(child) else {
            continue;
        };
        match el.value().name() {
            "br" => push_attribution(&mut out, first_link.take(), strong_text.take(), base),
            "a" if first_link.is_none() => {
                let href = el.attr("href").unwrap_or_default().to_string();
                first_link = Some((squash(&el.text().collect::<String>()), href));
            }
            "strong" => strong_text = Some(squash(&el.text().collect::<String>())),
            _ => {}
        }
    }
    push_attribution(&mut out, first_link, strong_text, base);

    out
}

fn push_attribution(
    out: &mut Vec<Attribution>,
    first_link: Option<(String, String)>,
    strong_text: Option<String>,
    base: &str,
) {
    let (Some((artist, href)), Some(title)) = (first_link, strong_text) else {
        return;
    };
    if artist == "Folk Music" {
        return;
    }
    out.push(Attribution {
        artist,
        artist_path: resolve_path(&href, base),
        title,
    });
}

/// The reference line is the one `<p>` whose text starts with `[`: Roud,
/// Child, Laws etc. as `Scheme Value` clauses separated by `;`, closed with
/// `trad.` when the song is traditional. Read from squashed text because the
/// clauses are link text mixed with plain text in whatever order the archive
/// wrote them.
fn parse_refs(article: &ElementRef) -> (SongRefs, Vec<Link>, bool) {
    let p_selector = Selector::parse("p").unwrap();
    let Some(reference) = article
        .select(&p_selector)
        .find(|p| p.text().collect::<String>().trim_start().starts_with('['))
    else {
        return (SongRefs::default(), Vec::new(), false);
    };

    let squashed = squash(&reference.text().collect::<String>());
    let inner = squashed
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let traditional = inner.contains("trad.");

    let mut refs = SongRefs::default();
    for part in inner.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("Master title:") {
            refs.master_title = Some(rest.trim().to_string());
        } else if let Some(rest) = part.strip_prefix("Ballad Index ") {
            if refs.ballad_index.is_none() {
                refs.ballad_index = Some(cut_at_slash(rest).to_string());
            }
        } else if let Some(rest) = part.strip_prefix("G/D ") {
            push_values(&mut refs.greig_duncan, rest);
        } else if let Some(rest) = part.strip_prefix("Roud ") {
            push_values(&mut refs.roud, rest);
        } else if let Some(rest) = part.strip_prefix("Child ") {
            push_values(&mut refs.child, rest);
        } else if let Some(rest) = part.strip_prefix("Laws ") {
            push_values(&mut refs.laws, rest);
        } else if let Some(rest) = part.strip_prefix("Henry ") {
            push_values(&mut refs.henry, rest);
        } else if let Some(rest) = part.strip_prefix("VWML ") {
            push_values(&mut refs.vwml, rest);
        }
    }

    let a_selector = Selector::parse("a").unwrap();
    let mut seen = HashSet::new();
    let external_links = reference
        .select(&a_selector)
        .filter_map(|a| {
            let href = a.attr("href")?;
            let is_external = href.starts_with("http")
                || a.value()
                    .has_class("external", CaseSensitivity::CaseSensitive);
            if !is_external || !seen.insert(href.to_string()) {
                return None;
            }
            Some(Link {
                text: squash(&a.text().collect::<String>()),
                url: href.to_string(),
            })
        })
        .collect();

    (refs, external_links, traditional)
}

/// Cuts a reference clause's value off before a trailing `/ Song Subject ...`
/// aside — the only place the archive appends unrelated prose to a scheme
/// value rather than starting a new `;`-separated clause.
fn cut_at_slash(value: &str) -> &str {
    match value.find(" / ") {
        Some(idx) => value[..idx].trim(),
        None => value.trim(),
    }
}

fn push_values(target: &mut Vec<String>, value: &str) {
    target.extend(
        cut_at_slash(value)
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from),
    );
}

/// `p.hiddenbibliography` is `Author: <cite>Title</cite>` runs; an author
/// with several books just has several `<cite>`s in a row with no author text
/// between them, so the last author text seen carries forward.
fn parse_bibliography(article: &ElementRef, base: &str) -> Vec<BookRef> {
    let selector = Selector::parse("p.hiddenbibliography").unwrap();
    let Some(bibliography) = article.select(&selector).next() else {
        return Vec::new();
    };

    let a_selector = Selector::parse("a").unwrap();
    let mut out = Vec::new();
    let mut current_author: Option<String> = None;
    let mut pending_text = String::new();

    for child in bibliography.children() {
        match child.value() {
            Node::Text(text) => pending_text.push_str(text),
            Node::Element(el) if el.name() == "cite" => {
                let author_text = squash(&pending_text);
                pending_text.clear();
                if !author_text.is_empty() {
                    current_author = Some(author_text.trim_end_matches(':').trim().to_string());
                }

                let cite = ElementRef::wrap(child).unwrap();
                out.push(BookRef {
                    author: current_author.clone(),
                    title: squash(&cite.text().collect::<String>()),
                    path: cite
                        .select(&a_selector)
                        .next()
                        .and_then(|a| a.attr("href"))
                        .map(|href| resolve_path(href, base)),
                });
            }
            _ => {}
        }
    }

    out
}

/// Each `<h3 id="...">` starts a lyrics block; its performer is the header
/// text up to the verb ("A.L. Lloyd sings *Scarborough Fair*"), and the block
/// runs through the following `<p>` siblings up to the next `<h3>`.
fn parse_lyrics(article: &ElementRef) -> Vec<LyricVersion> {
    let h3_selector = Selector::parse("h3[id]").unwrap();

    article
        .select(&h3_selector)
        .filter_map(|h3| {
            let anchor = h3.attr("id")?.to_string();
            let header_text = squash(&h3.text().collect::<String>());
            let performer = split_before_verb(&header_text).or_else(|| Some(header_text.clone()));

            let mut paragraphs = Vec::new();
            let mut sibling = h3.next_sibling();
            while let Some(node) = sibling {
                if let Some(el) = ElementRef::wrap(node) {
                    match el.value().name() {
                        "h3" => break,
                        "p" => paragraphs.push(paragraph_text(el)),
                        _ => {}
                    }
                }
                sibling = node.next_sibling();
            }

            if paragraphs.is_empty() {
                return None;
            }

            Some(LyricVersion {
                anchor,
                performer,
                text: paragraphs.join("\n\n"),
            })
        })
        .collect()
}

/// Every `<blockquote>` is a quotation; its attribution is the sentence in
/// the nearest preceding `<p>` when that sentence ends in a verb of
/// attribution ("...booklet noted:"), since that's the archive's only tell
/// for who is being quoted.
fn parse_notes(article: &ElementRef) -> Vec<Note> {
    let blockquote_selector = Selector::parse("blockquote").unwrap();
    let p_selector = Selector::parse("p").unwrap();

    article
        .select(&blockquote_selector)
        .filter_map(|blockquote| {
            let paragraphs: Vec<String> =
                blockquote.select(&p_selector).map(paragraph_text).collect();
            if paragraphs.is_empty() {
                return None;
            }
            Some(Note {
                attribution: preceding_attribution(blockquote),
                text: paragraphs.join("\n\n"),
            })
        })
        .collect()
}

const ATTRIBUTION_VERBS: [&str; 5] = ["noted:", "wrote:", "commented:", "said:", "notes:"];

fn preceding_attribution(blockquote: ElementRef) -> Option<String> {
    let mut sibling = blockquote.prev_sibling();
    while let Some(node) = sibling {
        if let Some(el) = ElementRef::wrap(node) {
            if el.value().name() != "p" {
                return None;
            }
            let text = squash(&el.text().collect::<String>());
            return ATTRIBUTION_VERBS
                .iter()
                .any(|verb| text.ends_with(verb))
                .then_some(text);
        }
        sibling = node.prev_sibling();
    }
    None
}

/// A recording is a prose `<p>` citing an album: any `<cite><a>` pointing at
/// a `/records/` page. One `<p>` can cite several albums (a recording and its
/// reissue), so this yields one [`Recording`] per `<cite>`, all sharing the
/// paragraph's context.
fn parse_recordings(article: &ElementRef, base: &str) -> Vec<Recording> {
    let p_selector = Selector::parse("p").unwrap();
    let cite_selector = Selector::parse("cite").unwrap();
    let a_selector = Selector::parse("a[href]").unwrap();

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for p in article.select(&p_selector) {
        if is_excluded_from_prose(p) {
            continue;
        }
        let context = squash(&p.text().collect::<String>());
        let performer = split_before_verb(&context);
        let year = find_year(&context);

        for cite in p.select(&cite_selector) {
            let Some(href) = cite.select(&a_selector).next().and_then(|a| a.attr("href")) else {
                continue;
            };
            if !href.contains("/records/") {
                continue;
            }
            let album_path = resolve_path(href, base);
            if !seen.insert(album_path.clone()) {
                continue;
            }
            out.push(Recording {
                performer: performer.clone(),
                album_title: Some(squash(&cite.text().collect::<String>())),
                album_path: Some(album_path),
                year: year.clone(),
                context: context.clone(),
            });
        }
    }

    out
}

/// Recording citations live in ordinary prose, not in the breadcrumb, the
/// bracketed reference line, the bibliography, or a quoted note.
fn is_excluded_from_prose(p: ElementRef) -> bool {
    let mut ancestor = p.parent();
    while let Some(node) = ancestor {
        if ElementRef::wrap(node).is_some_and(|el| el.value().name() == "blockquote") {
            return true;
        }
        ancestor = node.parent();
    }

    let classes = ["reference", "hiddenbibliography"];
    if classes
        .iter()
        .any(|c| p.value().has_class(c, CaseSensitivity::CaseSensitive))
    {
        return true;
    }

    p.text().collect::<String>().trim_start().starts_with('[')
}

/// Text up to the first "sang"/"sings"/"sing" verb, squashed — how the
/// archive's prose names a performer before saying what they did.
fn split_before_verb(text: &str) -> Option<String> {
    [" sang ", " sings ", " sing "]
        .into_iter()
        .filter_map(|verb| text.find(verb))
        .min()
        .map(|idx| squash(&text[..idx]))
        .filter(|s| !s.is_empty())
}

/// The first standalone `19xx`/`20xx` run in `text`. Standalone so a
/// catalogue number or track count never gets mistaken for a year.
fn find_year(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let run: String = chars[start..i].iter().collect();
        if run.len() == 4 && (run.starts_with("19") || run.starts_with("20")) {
            return Some(run);
        }
    }
    None
}

/// A `<p>`'s text with each `<br>` turned into a line break, everything else
/// squashed — the archive hard-wraps lyrics and quoted prose with `<br>`, and
/// nothing else in these blocks carries meaningful whitespace.
fn paragraph_text(p: ElementRef) -> String {
    let mut lines = Vec::new();
    let mut current = String::new();

    for node in p.descendants() {
        match node.value() {
            Node::Text(text) => current.push_str(text),
            Node::Element(el) if el.name() == "br" => {
                lines.push(squash(&current));
                current.clear();
            }
            _ => {}
        }
    }
    lines.push(squash(&current));
    lines.retain(|line| !line.is_empty());

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELFIN_KNIGHT: &str = include_str!("../../tests/fixtures/song_elfinknight.html");
    const REYNARDINE: &str = include_str!("../../tests/fixtures/song_reynardine.html");

    #[test]
    fn a_page_with_no_h1_is_a_parse_error() {
        let html =
            "<html><body><article id=\"mainArticle\"><p>Nothing here.</p></article></body></html>";
        let err = parse(html, "/nowhere.html").unwrap_err();
        assert!(matches!(err, Error::Parse { what: "song", .. }));
    }

    #[test]
    fn elfin_knight_carries_every_kind_of_reference() {
        let song = parse(ELFIN_KNIGHT, "/martin.carthy/songs/theelfinknight.html").unwrap();

        assert!(song.title.contains("Elfin Knight"));
        assert!(song.titles.len() >= 4);
        assert_eq!(song.refs.roud, ["12"]);
        assert_eq!(song.refs.child, ["2"]);
        assert_eq!(song.refs.master_title.as_deref(), Some("The Elfin Knight"));
        assert!(song.traditional);
        assert_eq!(song.refs.ballad_index.as_deref(), Some("C002"));

        let artists: Vec<&str> = song
            .attributions
            .iter()
            .map(|a| a.artist.as_str())
            .collect();
        assert!(artists.contains(&"Martin Carthy"));
        assert!(artists.contains(&"Shirley Collins"));

        let lloyd = song
            .lyrics
            .iter()
            .find(|l| l.anchor == "allloyd")
            .expect("allloyd lyrics block");
        assert!(
            lloyd
                .performer
                .as_deref()
                .unwrap_or_default()
                .contains("Lloyd")
        );
        assert!(lloyd.text.contains("Savoury, sage, rosemary and thyme"));

        assert!(!song.notes.is_empty());
        assert!(!song.recordings.is_empty());
        assert!(
            song.recordings
                .iter()
                .any(|r| r.album_path.as_deref().is_some_and(|p| p.starts_with('/')))
        );
    }

    #[test]
    fn reynardine_parses_without_error_and_has_lyrics() {
        let song = parse(REYNARDINE, "/lloyd/songs/themountainshigh.html").unwrap();
        assert!(!song.lyrics.is_empty());
        assert!(song.lyrics.iter().any(|l| !l.text.is_empty()));
    }
}
