//! Artist, discography and record-search parsers. Owned by the artist-parser work.
use scraper::{ElementRef, Html, Node, Selector};

use crate::client::resolve_path;
use crate::error::{Error, Result};
use crate::models::{Album, Artist, Source};
use crate::parse::squash;

/// Release formats the archive writes. Matched as a whole word, case-insensitively,
/// against a metadata tail — "as written" in [`Album::format`] means we keep the
/// casing found on the page rather than one of these.
const FORMATS: [&str; 9] = [
    "LP", "CD", "EP", "SP", "cassette", "MC", "78", "DVD", "download",
];

pub fn parse(html: &str, path: &str) -> Result<Artist> {
    let doc = Html::parse_document(html);
    // The page's own `<h1>` inside `#mainArticle`, not the site banner's — the
    // banner repeats "Mainly Norfolk: English Folk and Other Good Music" on every
    // page and comes first in document order.
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
            what: "artist",
            url: path.into(),
        });
    }
    let name = match h1 {
        Some(el) => squash(&el.text().collect::<String>()),
        None => strip_site_suffix(&squash(&title_tag.unwrap().text().collect::<String>())),
    };

    // The artist's own nav entry is whichever `div` under `nav#mainNav` isn't the
    // site-wide `#folk` block — its `a.biography`/`a.records`/`a.songs` links are
    // the artist's, where the `#folk` block's own `a.records` is the whole
    // archive's discography search instead.
    let nav_div_sel = Selector::parse("nav#mainNav > div").unwrap();
    let artist_div = doc
        .select(&nav_div_sel)
        .find(|div| div.value().attr("id") != Some("folk"));

    let biography_path = artist_div
        .and_then(|div| first_href_of_class(div, "biography"))
        .map(|href| resolve_path(&href, path));
    let discography_path = artist_div
        .and_then(|div| first_href_of_class(div, "records"))
        .map(|href| resolve_path(&href, path));
    let songs_path = artist_div
        .and_then(|div| first_href_of_class(div, "songs"))
        .map(|href| resolve_path(&href, path));

    Ok(Artist {
        path: path.to_string(),
        name,
        biography_path,
        discography_path,
        songs_path,
        source: Source::MainlyNorfolk,
    })
}

fn first_href_of_class(scope: ElementRef, class: &str) -> Option<String> {
    let selector = Selector::parse(&format!("a.{class}")).ok()?;
    scope
        .select(&selector)
        .next()
        .and_then(|a| a.value().attr("href"))
        .map(String::from)
}

/// Strips a trailing " - Mainly Norfolk"-style site suffix from a `<title>`, when
/// there is one. None of the archive's own titles carry one today, but a caller
/// falling back to `<title>` is exactly the case where a template might add one.
fn strip_site_suffix(title: &str) -> String {
    for sep in [" - ", " | ", " :: "] {
        if let Some((head, tail)) = title.rsplit_once(sep)
            && tail.to_lowercase().contains("mainly norfolk")
        {
            return head.trim().to_string();
        }
    }
    title.to_string()
}

/// A discography page's releases: `<h2>` year headings over `<p>` entries, each
/// naming a credited artist, one or more `<cite>` titles, and a comma-separated
/// tail of format, label/catalogue and year. Shared by [`discography`] (walking a
/// whole page of these) and by `album::parse` (reusing the same `<p>` shape for a
/// release documented inline rather than in its own table).
pub fn discography(html: &str, base: &str) -> Vec<Album> {
    let doc = Html::parse_document(html);
    let Ok(article_sel) = Selector::parse("article#mainArticle") else {
        return Vec::new();
    };
    let Some(article) = doc.select(&article_sel).next() else {
        return Vec::new();
    };
    let entry_sel = Selector::parse("h2, p").unwrap();
    let img_sel = Selector::parse("img").unwrap();

    let mut albums = Vec::new();
    let mut current_year: Option<String> = None;

    for el in article.select(&entry_sel) {
        match el.value().name() {
            "h2" => {
                let text = squash(&el.text().collect::<String>());
                if text.len() == 4 && text.bytes().all(|b| b.is_ascii_digit()) {
                    current_year = Some(text);
                }
            }
            "p" => albums.extend(release_entries(
                el,
                base,
                current_year.as_deref(),
                &img_sel,
                Source::MainlyNorfolk,
            )),
            _ => {}
        }
    }
    albums
}

/// Every `Album` a `<p>` entry describes — usually one, occasionally several when
/// a reissue note cites the albums it superseded in the same paragraph. Cites with
/// no `<a href>` (a title with nothing to link to, e.g. an out-of-print box set)
/// contribute nothing.
fn release_entries(
    p: ElementRef,
    base: &str,
    fallback_year: Option<&str>,
    img_sel: &Selector,
    source: Source,
) -> Vec<Album> {
    let cite_sel = Selector::parse("cite").unwrap();
    let cite_link_sel = Selector::parse("a[href]").unwrap();

    let linked: Vec<(String, String)> = p
        .select(&cite_sel)
        .filter_map(|cite| {
            let href = cite.select(&cite_link_sel).next()?.value().attr("href")?;
            Some((squash(&cite.text().collect::<String>()), href.to_string()))
        })
        .collect();
    if linked.is_empty() {
        return Vec::new();
    }

    let (before, tail) = split_around_cites(p);
    let artist = {
        let t = before.trim_end_matches(':').trim();
        (!t.is_empty()).then(|| t.to_string())
    };
    let meta = parse_release_tail(&tail, fallback_year);
    let cover_url = p
        .select(img_sel)
        .next()
        .and_then(|img| img.value().attr("src"))
        .map(|src| resolve_path(src, base));

    linked
        .into_iter()
        .map(|(title, href)| Album {
            path: resolve_path(&href, base),
            title,
            artist: artist.clone(),
            year: meta.year.clone(),
            label: meta.label.clone(),
            catalogue_number: meta.catalogue_number.clone(),
            format: meta.format.clone(),
            cover_url: cover_url.clone(),
            source,
        })
        .collect()
}

/// Splits a `<p>`'s direct children into the text before its first `<cite>` (the
/// credited artist) and the text after its last one (the format/label/year tail).
/// Text between several cites — reissue notes like "(reissue of ... and ...)" — is
/// deliberately dropped: it belongs to neither role.
///
/// When a `<p>` holds several cites, the tail is only ever read from after the
/// *last* one. This is correct, not a shortcut: those entries are reissue notes
/// citing the other releases a reissue drew on ("Round Up ... (reissue of half
/// of Martin Carthy and Second Album)"), not two releases sharing one entry — the
/// primary release's own metadata sits between the first cite and the note, and
/// the note itself carries none of its own worth keeping.
pub(crate) fn split_around_cites(p: ElementRef) -> (String, String) {
    let children: Vec<_> = p.children().collect();
    let last_cite_idx = children
        .iter()
        .rposition(|n| n.value().as_element().is_some_and(|e| e.name() == "cite"));

    let mut before = String::new();
    let mut tail = String::new();
    let mut past_first_cite = false;

    for (i, node) in children.iter().enumerate() {
        if node
            .value()
            .as_element()
            .is_some_and(|e| e.name() == "cite")
        {
            past_first_cite = true;
            continue;
        }
        let text = match node.value() {
            Node::Text(t) => t.text.to_string(),
            Node::Element(_) => ElementRef::wrap(*node)
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default(),
            _ => String::new(),
        };
        if !past_first_cite {
            before.push_str(&text);
        }
        if last_cite_idx.is_some_and(|idx| i > idx) {
            tail.push_str(&text);
        }
    }
    (squash(&before), squash(&tail))
}

/// Format, label, catalogue number and year parsed from a release's metadata.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReleaseMeta {
    pub year: Option<String>,
    pub format: Option<String>,
    pub label: Option<String>,
    pub catalogue_number: Option<String>,
}

/// Parses a comma-separated tail like `"LP, Topic 12TS340, 1965"` into its parts.
///
/// Only the first and last comma-segments are ever inspected for format and year:
/// scanning every segment would misread a catalogue number that happens to embed a
/// year-shaped run of digits (`"Topic 12TS2015"`) as the release year. Whatever's
/// left in the middle — normally one segment, occasionally two when an entry
/// writes a comma between label and catalogue (`"Decca, LK 4844"`) — is rejoined
/// and handed to [`split_label_and_catalogue`] as one string, so that stray comma
/// doesn't silently drop the catalogue number.
pub(crate) fn parse_release_tail(tail: &str, fallback_year: Option<&str>) -> ReleaseMeta {
    let tokens: Vec<&str> = tail
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();

    let mut format = None;
    let mut consumed_first = false;
    if let Some(first) = tokens.first()
        && let Some(f) = find_format(first)
    {
        format = Some(f.to_string());
        consumed_first = true;
    }

    let mut year = None;
    let mut consumed_last = false;
    if let Some(last) = tokens.last()
        && !(tokens.len() == 1 && consumed_first)
        && let Some(y) = find_year(last)
    {
        year = Some(y);
        consumed_last = true;
    }
    let year = year.or_else(|| fallback_year.map(String::from));

    let start = if consumed_first { 1 } else { 0 };
    let end = if consumed_last {
        tokens.len().saturating_sub(1)
    } else {
        tokens.len()
    };
    // Normally exactly one segment ("Decca LK 4545"), but a handful of entries
    // put a comma between label and catalogue ("Decca, LK 4844") — joining
    // every middle segment before splitting means that comma doesn't silently
    // drop the catalogue number.
    let (label, catalogue_number) = if start < end {
        split_label_and_catalogue(&tokens[start..end].join(" "))
    } else {
        (None, None)
    };

    ReleaseMeta {
        year,
        format,
        label,
        catalogue_number,
    }
}

/// The first word in `text` matching a known format, case-insensitively — kept as
/// written (`"LP"`, not the keyword's own casing) per [`Album::format`].
pub(crate) fn find_format(text: &str) -> Option<&str> {
    text.split_whitespace().find_map(|word| {
        let bare = word.trim_matches(|c: char| !c.is_alphanumeric());
        FORMATS
            .iter()
            .any(|f| f.eq_ignore_ascii_case(bare))
            .then_some(bare)
    })
}

/// The last 19xx/20xx year found in `text`, scanning left to right so the
/// rightmost match wins — the tail's year is always the last thing on the line.
pub(crate) fn find_year(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut found = None;
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if bytes[i..i + 4].iter().all(u8::is_ascii_digit)
            && (&bytes[i..i + 2] == b"19" || &bytes[i..i + 2] == b"20")
        {
            found = Some(text[i..i + 4].to_string());
            i += 4;
        } else {
            i += 1;
        }
    }
    found
}

/// Splits `"Topic Records TSCD340"` into label `"Topic Records"` and catalogue
/// number `"TSCD340"`.
///
/// Catalogue numbers are near-universally all-caps alphanumeric
/// (`LK 4545`, `EPK-801`, `TSCD707/8`); label names routinely aren't
/// (`"Topic Records"`, `"Folk Scene"`). So: walk words from the right while
/// each one is entirely uppercase letters/digits/`-`/`/`/`.`/`'`, and take that
/// trailing run as the catalogue number. A first pass tried "last word
/// containing a digit", but that splits a catalogue number in half whenever
/// it's itself `LETTERS NUMBERS` (`"Decca LK 4545"` gave label `"Decca LK"`,
/// catalogue `"4545"` — wrong; it's label `"Decca"`, catalogue `"LK 4545"`).
///
/// Two guards, and either failing means the whole text is the label with no
/// catalogue number: the trailing run must contain at least one digit (an
/// all-letter run like "UK" is a country code, not a catalogue number), and
/// at least one word must remain outside it (there must be a label left).
pub(crate) fn split_label_and_catalogue(text: &str) -> (Option<String>, Option<String>) {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return (None, None);
    }

    let is_catalogue_word = |w: &str| {
        !w.is_empty()
            && w.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || "-/.'".contains(c))
    };

    let mut split = words.len();
    while split > 0 && is_catalogue_word(words[split - 1]) {
        split -= 1;
    }
    let catalogue_words = &words[split..];
    let has_digit = catalogue_words
        .iter()
        .any(|w| w.chars().any(|c| c.is_ascii_digit()));

    if split == 0 || catalogue_words.is_empty() || !has_digit {
        let text = text.trim();
        return ((!text.is_empty()).then(|| text.to_string()), None);
    }

    let label = words[..split].join(" ");
    let catalogue_number = catalogue_words.join(" ");
    (Some(label), Some(catalogue_number))
}

/// Results of `records/search.php`, grouped as the archive groups them: one
/// credited artist per `<li>`, each of its links an [`Album`] with nothing but a
/// title, path and that shared artist — the search results carry no more.
pub fn records_search(html: &str, base: &str) -> Vec<(String, Vec<Album>)> {
    let doc = Html::parse_document(html);
    let Ok(li_sel) = Selector::parse("ul.plain li") else {
        return Vec::new();
    };
    let link_sel = Selector::parse("a[href]").unwrap();

    doc.select(&li_sel)
        .filter_map(|li| {
            let links: Vec<_> = li.select(&link_sel).collect();
            if links.is_empty() {
                return None;
            }

            let mut before = String::new();
            for node in li.children() {
                if node.value().is_element() {
                    break;
                }
                if let Some(text) = node.value().as_text() {
                    before.push_str(text);
                }
            }
            let artist = squash(before.trim_end_matches(':').trim());

            let albums = links
                .into_iter()
                .map(|a| Album {
                    path: resolve_path(a.value().attr("href").unwrap(), base),
                    title: squash(&a.text().collect::<String>()),
                    artist: (!artist.is_empty()).then(|| artist.clone()),
                    year: None,
                    label: None,
                    catalogue_number: None,
                    format: None,
                    cover_url: None,
                    source: Source::MainlyNorfolk,
                })
                .collect();
            Some((artist, albums))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTIST_INDEX: &str = include_str!("../../tests/fixtures/artist_index.html");
    const ARTIST_DISCOGRAPHY: &str = include_str!("../../tests/fixtures/artist_discography.html");
    const LABEL_TOPIC: &str = include_str!("../../tests/fixtures/label_topic.html");
    const SEARCH_RECORDS: &str = include_str!("../../tests/fixtures/search_records.html");

    #[test]
    fn artist_index_gives_name_and_nav_paths() {
        let artist = parse(ARTIST_INDEX, "/martin.carthy/").unwrap();
        assert!(artist.name.contains("Martin Carthy"));
        assert_eq!(
            artist.discography_path.as_deref(),
            Some("/martin.carthy/records/")
        );
        assert_eq!(artist.songs_path.as_deref(), Some("/martin.carthy/songs/"));
        assert_eq!(
            artist.biography_path.as_deref(),
            Some("/martin.carthy/biography.html")
        );
    }

    #[test]
    fn discography_parses_a_full_page() {
        let albums = discography(ARTIST_DISCOGRAPHY, "/martin.carthy/records/index.html");
        assert!(albums.len() >= 100, "got {}", albums.len());
        assert!(albums.iter().any(|a| a.year.as_deref() == Some("1965")));
        // The catalogue number is the trailing all-caps run, not just the last
        // digit-bearing word — "LK 4545" stays together rather than splitting.
        assert!(albums.iter().any(|a| a.label.as_deref() == Some("Decca")
            && a.catalogue_number.as_deref() == Some("LK 4545")));
        // The no-space form ("TL5368") is the one most likely to regress.
        assert!(albums.iter().any(|a| a.label.as_deref() == Some("Fontana")
            && a.catalogue_number.as_deref() == Some("TL5368")));
        // "LP, Decca, LK 4844, 1967" — a stray comma between label and
        // catalogue must not make the catalogue number vanish.
        assert!(albums.iter().any(|a| a.label.as_deref() == Some("Decca")
            && a.catalogue_number.as_deref() == Some("LK 4844")));
        assert!(albums.iter().all(|a| a.path.starts_with('/')));
    }

    #[test]
    fn label_discography_parses_a_full_page() {
        let albums = discography(LABEL_TOPIC, "/folk/records/topic.html");
        assert!(albums.len() >= 100, "got {}", albums.len());
        assert!(albums.iter().all(|a| !a.title.is_empty()));
    }

    #[test]
    fn records_search_groups_by_credited_artist() {
        let groups = records_search(SEARCH_RECORDS, "/folk/records/search.php");
        assert!(groups.len() >= 3);
        // Several credited-artist groups contain "Eliza Carthy" as a substring
        // ("Eliza Carthy & Nancy Kerr", "Martin and Eliza Carthy", ...); the one
        // actually credited to her alone is the one with >= 10 albums.
        assert!(
            groups
                .iter()
                .any(|(artist, albums)| artist.contains("Eliza Carthy") && albums.len() >= 10)
        );
    }
}
