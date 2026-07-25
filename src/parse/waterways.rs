//! waterwaysongs.info parsers. Owned by the waterways-parser work.
//!
//! The site is a Xara Web Designer export: every visible line is an
//! absolutely-positioned `<span>` whose class list is the full chain of
//! paragraph/character styles that produced it, so one element routinely
//! carries several of our markers at once — a lyric line is also "Author",
//! and the "recorded by" credit is also "Lyrics" (a quirk of how Xara names
//! the cascade, not an actual verse). Matching on marker-contains alone
//! over-collects, so the filters below narrow it back down to what the
//! fixture actually shows. Xara also emits one full copy of the article per
//! responsive breakpoint (desktop, then an identical mobile layout), so
//! extraction is scoped to the first such copy to avoid doubling every field.

use std::collections::HashSet;

use scraper::{ElementRef, Html, Selector};

use crate::client::resolve_path;
use crate::error::{Error, Result};
use crate::models::{Source, WaterwaysSong};

use super::squash;

fn class_has(el: &ElementRef, marker: &str) -> bool {
    el.attr("class").is_some_and(|c| c.contains(marker))
}

fn line(el: &ElementRef) -> String {
    squash(&el.text().collect::<String>())
}

/// Whether `el` sits inside another element (below `scope`) that itself
/// matches. Notes and the recorded-by credit nest emphasis/name spans inside
/// their own line, and the nested spans carry the same marker class as the
/// line — this keeps only the outermost match, so a line isn't also counted
/// again as its own bold word.
fn nested_in_match(
    scope: ElementRef,
    el: ElementRef,
    matches: impl Fn(&ElementRef) -> bool,
) -> bool {
    el.ancestors()
        .take_while(|n| n.id() != scope.id())
        .filter_map(ElementRef::wrap)
        .any(|a| matches(&a))
}

pub fn song(html: &str, path: &str) -> Result<WaterwaysSong> {
    let doc = Html::parse_document(html);

    let title_selector = Selector::parse("title").unwrap();
    let Some(title_el) = doc.select(&title_selector).next() else {
        return Err(Error::Parse {
            what: "waterways song",
            url: path.into(),
        });
    };
    let title = line(&title_el);

    let div_selector = Selector::parse("div").unwrap();
    let scope = doc
        .select(&div_selector)
        .find(|el| class_has(el, "xr_xrc"))
        .unwrap_or_else(|| doc.root_element());

    let span_selector = Selector::parse("span").unwrap();
    let spans: Vec<ElementRef> = scope.select(&span_selector).collect();

    let author = spans
        .iter()
        .find(|el| class_has(el, "XX-95Author"))
        .map(line)
        .map(|s| s.strip_prefix("by ").unwrap_or(&s).to_string())
        .filter(|s| !s.is_empty());

    let is_lyrics = |el: &ElementRef| {
        class_has(el, "XX-95Lyrics")
            && !class_has(el, "XX-95recorded-95by")
            && !class_has(el, "XX-95recordings_list")
    };
    let lyric_lines: Vec<String> = spans.iter().filter(|el| is_lyrics(el)).map(line).collect();
    let lyrics = (!lyric_lines.is_empty()).then(|| lyric_lines.join("\n"));

    let is_notes = |el: &ElementRef| class_has(el, "XX-95Notes");
    let notes: Vec<String> = spans
        .iter()
        .filter(|el| is_notes(el) && !nested_in_match(scope, **el, is_notes))
        .map(line)
        .filter(|s| !s.is_empty())
        .filter(|s| !s.starts_with("Recorded on"))
        .filter(|s| author.as_deref() != Some(s.as_str()))
        .filter(|s| !lyric_lines.contains(s))
        .collect();

    let is_recorded = |el: &ElementRef| {
        class_has(el, "XX-95recorded-95by") || class_has(el, "XX-95recordings_list")
    };
    let mut recorded_by = Vec::new();
    for el in spans
        .iter()
        .filter(|el| is_recorded(el) && !nested_in_match(scope, **el, is_recorded))
    {
        let text = line(el);
        if !text.is_empty() && !recorded_by.contains(&text) {
            recorded_by.push(text);
        }
    }

    let audio_selector = Selector::parse("audio").unwrap();
    let source_selector = Selector::parse("source").unwrap();
    let audio_url = doc.select(&audio_selector).next().and_then(|audio| {
        let src = audio.attr("src").or_else(|| {
            audio
                .select(&source_selector)
                .next()
                .and_then(|s| s.attr("src"))
        })?;
        let resolved = resolve_path(src, path);
        Some(
            if resolved.starts_with("http://") || resolved.starts_with("https://") {
                resolved
            } else {
                format!("{}{resolved}", Source::Waterways.base_url())
            },
        )
    });

    Ok(WaterwaysSong {
        path: path.into(),
        title,
        author,
        lyrics,
        notes,
        recorded_by,
        audio_url,
    })
}

/// The song menu: every `/Songs/` link is a title, an author credit, or a
/// blank separator, and the whole group repeats across the menu's several
/// layout copies. The title link always comes first for a given song, so
/// first-seen-wins dedup by path keeps the title and drops the rest.
pub fn index(html: &str) -> Vec<WaterwaysSong> {
    let doc = Html::parse_document(html);
    let selector = Selector::parse("a").unwrap();

    let mut seen = HashSet::new();
    let mut songs = Vec::new();
    for a in doc.select(&selector) {
        let Some(href) = a.attr("href") else { continue };
        if !href.contains("/Songs/") {
            continue;
        }
        let title = line(&a);
        if title.is_empty() {
            continue;
        }
        let path = resolve_path(href, "/songmenu.htm");
        if seen.insert(path.clone()) {
            songs.push(WaterwaysSong {
                path,
                title,
                author: None,
                lyrics: None,
                notes: Vec::new(),
                recorded_by: Vec::new(),
                audio_url: None,
            });
        }
    }
    songs
}

#[cfg(test)]
mod tests {
    use super::*;

    const SONG_HTML: &str = include_str!("../../tests/fixtures/waterways_song.html");
    const MENU_HTML: &str = include_str!("../../tests/fixtures/waterways_menu.html");

    #[test]
    fn parses_hard_working_boater() {
        let song = song(SONG_HTML, "/Songs/H/hard_working.htm").unwrap();
        assert_eq!(song.title, "Hard Working Boater");
        assert_eq!(song.author.as_deref(), Some("David Blagrove"));

        let lyrics = song.lyrics.expect("lyrics");
        assert!(lyrics.contains("I'm a hard working boater and sharp as a knife"));
        assert!(lyrics.contains('\n'));

        assert_eq!(
            song.audio_url.as_deref(),
            Some("https://www.waterwaysongs.info/sounds/hard_working_boater_song.mp3")
        );
    }

    #[test]
    fn separates_notes_from_lyrics_and_the_recorded_by_credit() {
        let song = song(SONG_HTML, "/Songs/H/hard_working.htm").unwrap();
        let lyrics = song.lyrics.unwrap();

        // The "Recorded by" credit shares a class with the lyrics
        // ("XX-95Lyrics_a_a") but is neither a lyric nor a note.
        assert!(!lyrics.contains("Recorded by"));
        assert!(!song.notes.iter().any(|n| n.contains("Recorded by")));
        assert!(song.recorded_by.iter().any(|r| r.contains("The Boatmen")));

        // Glossary and sleeve-note commentary land as notes, not lyrics.
        assert!(song.notes.iter().any(|n| n.contains("lock 46")));
        assert!(!lyrics.contains("lock 46"));

        // The "Recorded on :" heading precedes an empty recordings table and
        // carries nothing worth keeping.
        assert!(!song.notes.iter().any(|n| n.starts_with("Recorded on")));

        // Xara duplicates the whole page per responsive breakpoint; only the
        // first copy should be counted.
        assert_eq!(lyrics.lines().count(), 24);
    }

    #[test]
    fn song_without_a_title_is_a_parse_error() {
        let err = song("<html><body>no title here</body></html>", "/Songs/X/x.htm").unwrap_err();
        assert!(matches!(
            err,
            Error::Parse {
                what: "waterways song",
                ..
            }
        ));
    }

    #[test]
    fn indexes_the_song_menu() {
        let songs = index(MENU_HTML);

        assert!(
            songs.len() >= 300,
            "expected at least 300 unique songs, got {}",
            songs.len()
        );

        let mut paths = HashSet::new();
        for song in &songs {
            assert!(!paths.contains(&song.path), "duplicate path: {}", song.path);
            paths.insert(song.path.clone());
            assert!(
                song.path.starts_with("/Songs/"),
                "not site-absolute: {}",
                song.path
            );
            assert!(!song.title.is_empty());
        }

        let boater = songs.iter().find(|s| s.path == "/Songs/H/hard_working.htm");
        assert_eq!(
            boater.map(|s| s.title.as_str()),
            Some("Hard Working Boater")
        );
    }
}
