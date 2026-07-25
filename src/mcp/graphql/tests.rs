//! Schema-level tests. None of these touch the network.
//!
//! Two things are worth asserting without a live archive. The first is that the
//! guardrails hold before a resolver ever runs — depth is a validation-time
//! rejection, so a query that would walk the graph forever costs zero requests.
//! The second is that the SDL carries the guidance a caller needs: the tool
//! returns the schema and nothing else, so anything the schema doesn't say, a
//! model composing a query never learns.

use super::connection::{MAX_PAGE, PageArgs, page_of};
use super::{FolkSchema, build_schema};

fn schema() -> FolkSchema {
    build_schema()
}

/// Every type and field the MCP promises. A rename that slips through here is a
/// silently broken query for anyone who saved one.
#[test]
fn the_sdl_declares_the_whole_graph() {
    let sdl = schema().sdl();

    for type_name in [
        "type Song",
        "type SongSummary",
        "type SongRefs",
        "type Attribution",
        "type Recording",
        "type Artist",
        "type Album",
        "type Track",
        "type Label",
        "type WaterwaysSong",
        "type ArtistRecords",
        "type Page",
        "type LyricVersion",
        "type Note",
        "type BookRef",
        "type Link",
        "enum Source",
        "enum RefScheme",
    ] {
        assert!(
            sdl.contains(type_name),
            "the SDL no longer declares {type_name}"
        );
    }

    for connection in [
        "type SongSummaryConnection",
        "type RecordingConnection",
        "type AlbumConnection",
        "type LabelConnection",
        "type WaterwaysSongConnection",
        "type ArtistRecordsConnection",
    ] {
        assert!(
            sdl.contains(connection),
            "the SDL no longer declares {connection}"
        );
    }
}

#[test]
fn the_sdl_declares_every_root_field() {
    let sdl = schema().sdl();
    for field in [
        "song(",
        "songs(",
        "childBallads(",
        "lawsBallads(",
        "artist(",
        "records(",
        "album(",
        "labels(",
        "waterwaysSongs(",
        "waterwaysSong(",
        "page(",
    ] {
        assert!(
            sdl.contains(field),
            "the SDL no longer has a root `{field}`"
        );
    }
}

#[test]
fn the_lazy_edges_are_all_there() {
    let sdl = schema().sdl();
    // The cycle: a song's recordings reach albums, whose tracks reach songs.
    for edge in [
        "attributions: [Attribution!]!",
        "recordings(",
        "artist: Artist",
        "album: Album",
        "song: Song",
        "tracks: [Track!]!",
        "biography: Page",
        "discography(",
        "albums(",
    ] {
        assert!(
            sdl.contains(edge),
            "the SDL no longer has the edge `{edge}`"
        );
    }
}

/// The tool hands back the SDL and nothing else — no hand-written prelude. That
/// only works if the schema itself says what a query costs, so the load-bearing
/// warnings are asserted rather than assumed.
#[test]
fn the_sdl_says_what_the_expensive_edges_cost() {
    let sdl = schema().sdl();
    for needle in [
        // The one that turns a list into a hundred requests.
        "Fetches one page per summary",
        // Tracks and albums, the other two per-item fetches.
        "Fetches this album's page",
        "Fetches that page",
        // What is free, so a caller isn't scared off the cheap fields too.
        "Free —",
        "is then free",
        // Paging: what a cursor is, the sizes, and that counting is free.
        "A cursor is the item's `path`",
        "default page is 25 and the cap is 100",
        "Free to ask for",
        // The concurrency budget, which is the real ceiling on one query.
        "four at a time",
        // The search endpoint being the right way in, rather than the index.
        "670KB",
        // The filter grammar, which has no other documentation.
        "inclusive range",
    ] {
        assert!(
            sdl.contains(needle),
            "the SDL no longer states {needle:?} — a caller reading only the schema \
             would not know it"
        );
    }
}

#[test]
fn the_schema_is_read_only() {
    let sdl = schema().sdl();
    assert!(
        !sdl.contains("type Mutation"),
        "the archives are public websites"
    );
    assert!(!sdl.contains("type Subscription"));
}

#[tokio::test]
async fn introspection_answers_without_touching_an_archive() {
    let response = schema()
        .execute("{ __schema { queryType { name } types { name } } }")
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);

    let data = response.data.into_json().unwrap();
    assert_eq!(data["__schema"]["queryType"]["name"], "Query");
    let types = data["__schema"]["types"].as_array().unwrap();
    assert!(types.iter().any(|t| t["name"] == "Song"));
}

#[tokio::test]
async fn a_query_deeper_than_the_cap_is_rejected_before_it_fetches_anything() {
    // The cycle: song → recordings → nodes → album → tracks → song → …
    // Rejected during validation, so no resolver runs and nothing is fetched —
    // which is why this test can exist without a mock archive.
    let deep = format!(
        r#"{{ song(path: "/x.html") {{ {} title {} }} }}"#,
        "recordings { nodes { album { tracks { song { ".repeat(4),
        "} } } } }".repeat(4)
    );
    let response = schema().execute(&deep).await;

    let message = response
        .errors
        .first()
        .map(|e| e.message.to_lowercase())
        .unwrap_or_default();
    assert!(message.contains("too deep"), "{:?}", response.errors);
}

#[tokio::test]
async fn a_shallow_query_passes_validation() {
    // The other half of the depth test: the cap must not reject ordinary work.
    // Validation only — the resolver would fetch, so this asserts on the error
    // being absent from *validation*, which is what an unknown field would give.
    let response = schema()
        .execute("{ __type(name: \"Song\") { fields { name } } }")
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
}

#[tokio::test]
async fn unknown_fields_are_rejected_rather_than_ignored() {
    let response = schema()
        .execute("{ song(path: \"/x.html\") { nonsense } }")
        .await;
    assert!(!response.errors.is_empty());
}

#[test]
fn a_page_is_clamped_to_the_maximum() {
    // Whole-index fields hand back thousands of rows; the cap is what stands
    // between `first: 9999` and serialising the archive.
    let items: Vec<String> = (0..500).map(|i| i.to_string()).collect();
    let page = page_of(
        items,
        PageArgs {
            after: None,
            before: None,
            first: Some(9999),
            last: None,
        },
        Clone::clone,
    )
    .unwrap();

    assert_eq!(page.items.len(), MAX_PAGE as usize);
    assert_eq!(page.total, 500);
    assert!(page.has_next);
}
