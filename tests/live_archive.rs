//! Checks against the real archives.
//!
//! `#[ignore]` by design: these hit a volunteer-run website, so they are not
//! part of `cargo test` and never run in CI. They exist to answer the one
//! question a fixture cannot — "does the archive still serve what we assume it
//! serves" — and are meant to be run by hand when something looks wrong:
//!
//! ```sh
//! cargo test --test live_archive -- --ignored --test-threads=1
//! ```
//!
//! `--test-threads=1` matters. Each test builds its own client, so the shared
//! concurrency cap does not apply across them; running them serially keeps the
//! whole suite to one request at a time.

use mainlynorfolk_mcp::archive::{Archive, SongQuery, paths};
use mainlynorfolk_mcp::client::Client;
use mainlynorfolk_mcp::models::Source;

fn archive() -> Archive {
    Archive::new(Client::new().expect("client"))
}

/// The endpoints every other lookup is built on still answer.
#[tokio::test]
#[ignore = "hits the live archive"]
async fn the_entry_points_are_all_still_there() {
    let client = Client::new().expect("client");
    for path in [
        paths::FOLK,
        paths::SONG_INDEX,
        paths::CHILD_INDEX,
        paths::LAWS_INDEX,
        paths::RECORDS_INDEX,
        paths::LATEST_CHANGES,
        paths::BOOKS,
    ] {
        let html = client
            .get(Source::MainlyNorfolk, path)
            .await
            .unwrap_or_else(|e| panic!("{path} unreachable: {e}"));
        assert!(
            html.contains("mainArticle"),
            "{path} is not an archive page"
        );
    }

    let menu = client
        .get(Source::Waterways, paths::WATERWAYS_MENU)
        .await
        .expect("waterways menu");
    assert!(menu.contains("/Songs/"), "waterways menu has no song links");
}

/// The archive's own search still works and still beats fetching the index.
///
/// This is the assumption the whole lookup strategy rests on: if `search.php`
/// ever goes away, every search silently becomes a 670KB download.
#[tokio::test]
#[ignore = "hits the live archive"]
async fn song_search_answers_and_stays_small() {
    let client = Client::new().expect("client");
    let results = client
        .post_form(
            Source::MainlyNorfolk,
            paths::SONG_SEARCH,
            &[("song", "reynardine")],
        )
        .await
        .expect("song search");

    assert!(results.contains("Search Results"), "no results section");
    assert!(
        results.contains("themountainshigh"),
        "Reynardine no longer resolves to The Mountains High"
    );

    let index = client
        .get(Source::MainlyNorfolk, paths::SONG_INDEX)
        .await
        .expect("song index");
    assert!(
        results.len() * 4 < index.len(),
        "search ({} bytes) is no longer meaningfully cheaper than the index ({} bytes)",
        results.len(),
        index.len()
    );
}

/// Searching by reference number, which is how a model asks for a ballad it
/// knows only by its Child or Roud number.
#[tokio::test]
#[ignore = "hits the live archive"]
async fn a_reference_number_search_still_resolves() {
    let html = archive()
        .client()
        .post_form(
            Source::MainlyNorfolk,
            paths::SONG_SEARCH,
            &[("key", "roud"), ("keyval", "397")],
        )
        .await
        .expect("roud search");
    assert!(
        html.contains("themountainshigh"),
        "Roud 397 no longer found"
    );
}

/// An empty query never reaches the network — the endpoint answers it with the
/// entire index, which is exactly what this tool exists to avoid.
#[tokio::test]
async fn an_empty_search_is_refused_locally() {
    let err = archive()
        .search_songs(&SongQuery::default())
        .await
        .expect_err("empty query must be refused");
    assert!(err.to_string().contains("needs a title"), "{err}");
}

/// A model-composed path must not be able to turn this into an open proxy.
#[tokio::test]
async fn foreign_hosts_are_refused_without_a_request() {
    let err = Client::new()
        .expect("client")
        .get(Source::MainlyNorfolk, "https://example.com/")
        .await
        .expect_err("foreign host must be refused");
    assert!(
        err.to_string().contains("not a path on any archive"),
        "{err}"
    );
}

/// The second fetch of a page is served from disk rather than asked for again.
#[tokio::test]
#[ignore = "hits the live archive"]
async fn a_page_is_only_fetched_once() {
    let client = Client::new().expect("client");
    let first = client
        .get(Source::MainlyNorfolk, paths::CHILD_INDEX)
        .await
        .expect("first fetch");

    let started = std::time::Instant::now();
    let second = client
        .get(Source::MainlyNorfolk, paths::CHILD_INDEX)
        .await
        .expect("second fetch");
    let elapsed = started.elapsed();

    assert_eq!(first, second);
    // A disk read is sub-millisecond; anything near network latency means the
    // cache is not being consulted.
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "second fetch took {elapsed:?} — cache is not being used"
    );
}
