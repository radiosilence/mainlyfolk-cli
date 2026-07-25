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
//!
//! Some pages also carry a hidden "chords" popup — a whole second copy of the
//! lyrics with `[D]`/`[Bm]`-style chord symbols spliced in, toggled by a JS
//! button and marked `display: none` until then. That's a duplicate of the
//! visible lyrics, not additional content, so hidden markup is dropped before
//! any field is extracted from it.

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

/// Drops a trailing copyright notice from an author line, e.g. "Peter
/// Clement © 2019" -> "Peter Clement". Real information, but not part of the
/// name, and it would otherwise poison any grouping by author.
fn strip_copyright(s: &str) -> String {
    let paren_c = s
        .as_bytes()
        .windows(3)
        .position(|w| w[0] == b'(' && w[1].eq_ignore_ascii_case(&b'c') && w[2] == b')');
    match s.find('©').or(paren_c) {
        Some(i) => s[..i].trim().to_string(),
        None => s.to_string(),
    }
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

/// Whether `el`, or an ancestor up to `scope`, is inline-styled
/// `display: none` — a JS-toggled popup rather than default page content.
fn is_hidden(scope: ElementRef, el: ElementRef) -> bool {
    let styled_hidden = |e: &ElementRef| {
        e.attr("style")
            .is_some_and(|s| s.replace(' ', "").contains("display:none"))
    };
    styled_hidden(&el)
        || el
            .ancestors()
            .take_while(|n| n.id() != scope.id())
            .filter_map(ElementRef::wrap)
            .any(|a| styled_hidden(&a))
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
    let spans: Vec<ElementRef> = scope
        .select(&span_selector)
        .filter(|el| !is_hidden(scope, *el))
        .collect();

    let author = spans
        .iter()
        .find(|el| class_has(el, "XX-95Author"))
        .map(line)
        .map(|s| s.strip_prefix("by ").unwrap_or(&s).to_string())
        .map(|s| strip_copyright(&s))
        .filter(|s| !s.is_empty());

    // A heading that transitions into the notes ("Notes from the song
    // writer :") sometimes carries a stray "XX-95Lyrics" class from Xara's
    // style cascade even though its own text is tagged "XX-95Notes" — so
    // lyrics matching also excludes anything with a Notes-marked descendant.
    let is_lyrics = |el: &ElementRef| {
        class_has(el, "XX-95Lyrics")
            && !class_has(el, "XX-95recorded-95by")
            && !class_has(el, "XX-95recordings_list")
            && !el
                .select(&span_selector)
                .any(|d| class_has(&d, "XX-95Notes"))
    };
    let lyric_lines: Vec<String> = spans
        .iter()
        .filter(|el| is_lyrics(el) && !nested_in_match(scope, **el, is_lyrics))
        .map(line)
        .collect();
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
    const ALICE_WHITE_HTML: &str =
        include_str!("../../tests/fixtures/waterways_song_alice_white.html");
    const LUCY_MEGAN_HTML: &str =
        include_str!("../../tests/fixtures/waterways_song_lucy_megan.html");

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
    fn parses_across_multiple_song_pages() {
        // A parser validated against exactly one page is validated against
        // that page's quirks. These two are structurally the same as the
        // main fixture (two Xara breakpoint copies) but exercise different
        // markup: Alice White has a plain author line, Lucy Megan has a
        // trailing copyright and a nested chord overlay in its lyrics.
        for (html, path) in [
            (SONG_HTML, "/Songs/H/hard_working.htm"),
            (ALICE_WHITE_HTML, "/Songs/A/alice_white.htm"),
            (LUCY_MEGAN_HTML, "/Songs/A/lucy_megan.htm"),
        ] {
            let song = song(html, path).unwrap();
            assert!(!song.title.is_empty(), "{path}: empty title");
            assert!(song.author.is_some(), "{path}: no author");

            let lyrics = song.lyrics.as_deref().unwrap_or_default();
            assert!(!lyrics.is_empty(), "{path}: no lyrics");

            // Guards the responsive-breakpoint scoping: if the desktop and
            // mobile copies both leaked through, the opening line would
            // appear twice.
            let first_line = lyrics.lines().next().unwrap();
            assert_eq!(
                lyrics.lines().filter(|l| *l == first_line).count(),
                1,
                "{path}: opening lyric line repeated — breakpoint scoping regressed?"
            );
        }
    }

    #[test]
    fn strips_a_trailing_copyright_notice_from_the_author() {
        let song = song(LUCY_MEGAN_HTML, "/Songs/A/lucy_megan.htm").unwrap();
        assert_eq!(song.author.as_deref(), Some("Peter Clement"));
    }

    #[test]
    fn strip_copyright_recognises_both_notice_forms() {
        assert_eq!(strip_copyright("Jane Doe © 2020"), "Jane Doe");
        assert_eq!(strip_copyright("Jane Doe (c) 2020"), "Jane Doe");
        assert_eq!(strip_copyright("Jane Doe (C) 2020"), "Jane Doe");
        assert_eq!(strip_copyright("Jane Doe"), "Jane Doe");
    }

    #[test]
    fn hidden_chords_popup_is_excluded_rather_than_duplicating_the_lyrics() {
        // Lucy Megan has a second, hidden copy of its lyrics with chord
        // symbols spliced in ([D], [Bm], ...), toggled by a "chords" button
        // and marked `display: none` until then. It must not surface at all:
        // not as extra duplicate verse lines, and not as chord-only
        // fragments ("[D]" as its own entry) from the popup's nested spans.
        let song = song(LUCY_MEGAN_HTML, "/Songs/A/lucy_megan.htm").unwrap();
        let lyrics = song.lyrics.unwrap();

        assert!(
            !lyrics.contains('['),
            "hidden chords popup leaked into the lyrics"
        );
        assert_eq!(
            lyrics
                .lines()
                .filter(|l| *l == "You slip your morning mooring as the mist begins to lift")
                .count(),
            1,
            "the opening line should appear once, not once plain and once from the chords popup"
        );
    }

    #[test]
    fn a_notes_heading_mistagged_as_lyrics_stays_out_of_the_lyrics() {
        // "Notes from the song writer :" is the transition into Lucy Megan's
        // notes, but its wrapping span inherits an "XX-95Lyrics" class from
        // Xara's style cascade even though the text itself is tagged
        // "XX-95Notes".
        let song = song(LUCY_MEGAN_HTML, "/Songs/A/lucy_megan.htm").unwrap();
        assert!(!song.lyrics.unwrap().contains("Notes from the song writer"));
        assert!(
            song.notes
                .iter()
                .any(|n| n.contains("Notes from the song writer"))
        );
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
