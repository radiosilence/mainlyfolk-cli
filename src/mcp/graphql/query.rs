//! GraphQL query resolvers — the entry points into the graph.
//!
//! Each root field is one archive request. What it costs beyond that is decided
//! by which edges the selection walks into, and those say so themselves.

use async_graphql::{Context, Object, Result};

use super::connection::{ListConnection, PageArgs, page_complexity, paginate};
use super::loaders::{IndexKind, RecordSearches, SongSearch, SongSearches, to_gql_error};
use super::types::*;
use crate::models::{SongRefs, SongSummary, Source};

pub struct QueryRoot;

/// The archives, as one graph.
///
/// **What things cost.** There is no bulk endpoint: every page is its own HTTP
/// request to a hand-maintained website, and requests go out four at a time on
/// purpose. Each field says whether it is free or a fetch — read those before
/// selecting an edge across a whole list, because that is how one query becomes
/// two hundred requests. Within a single query a page is fetched once however
/// many times the selection reaches it, so following a cycle back to where you
/// started is free.
///
/// **Paging.** Lists are Relay connections: `nodes { ... }` for the items,
/// `first`/`after` to page. The default page is 25 and the cap is 100 — a larger
/// `first` is clamped, not refused. A cursor is the item's `path`, so
/// `after: "<the last path you saw>"` resumes; lists whose items have no path of
/// their own use their position instead. `totalCount` is free everywhere: these
/// lists arrive whole from one request, so it is a length rather than more work.
#[Object(name = "Query")]
#[allow(clippy::too_many_arguments)]
impl QueryRoot {
    /// One song page in full — titles, references, lyrics, notes, recordings,
    /// bibliography. One request.
    ///
    /// `path` is the id the rest of the schema hands you: `songs`, an index, a
    /// track, an artist's song list. Null when the archive has no such page.
    async fn song(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Site-absolute path, e.g. /martin.carthy/songs/theelfinknight.html")]
        path: String,
    ) -> Result<Option<GqlSong>> {
        load_song(ctx, &path).await
    }

    /// Search songs through the archive's own search endpoint.
    ///
    /// The right way to look a song up: the server filters, so a title query
    /// costs one small response where reading the full index would be 670KB.
    /// The arguments combine — a title *and* an author narrows. At least one of
    /// `title`, `author` or `number` is required, since the endpoint answers an
    /// empty search with the entire index.
    ///
    /// Results are summaries. Select `song { ... }` on them only where you need
    /// the page itself: that is a fetch each.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn songs(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Title or part of one. A trailing `*` is a wildcard, server-side.")]
        title: Option<String>,
        #[graphql(desc = "Author or performer name, or part of one.")] author: Option<String>,
        #[graphql(desc = "Reference scheme for `number`. Defaults to Roud, the archive's own.")]
        scheme: Option<RefScheme>,
        #[graphql(desc = "Reference number, e.g. \"12\" for Roud 12, \"P15\" for Laws P15.")]
        number: Option<String>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlSongSummary>> {
        let search = SongSearch {
            title: non_empty(title),
            author: non_empty(author),
            scheme,
            number: non_empty(number),
        };
        if search.to_query().is_empty() {
            return Err(async_graphql::Error::new(
                "A song search needs a title, an author, or a reference number.",
            ));
        }

        let found = ctx
            .data::<SongSearches>()?
            .load_one(search)
            .await
            .map_err(to_gql_error)?
            .unwrap_or_default();

        paginate(
            found.iter().cloned().map(GqlSongSummary::from).collect(),
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |s| s.0.path.clone(),
        )
    }

    /// The Child ballads the archive covers — Francis James Child's canonical
    /// 305. One request for the whole index, then filtered here.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn child_ballads(
        &self,
        ctx: &Context<'_>,
        #[graphql(
            desc = "A number (`84`), an inclusive range (`1-50`), a code prefix (`C`), or free \
                    text matched against the titles. Omit for the whole index."
        )]
        filter: Option<String>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlSongSummary>> {
        ballad_index(
            ctx,
            IndexKind::Child,
            filter,
            PageArgs {
                after,
                before,
                first,
                last,
            },
        )
        .await
    }

    /// The Laws index — American ballads of British origin, coded by letter
    /// series (`P15`). One request for the whole index, then filtered here.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn laws_ballads(
        &self,
        ctx: &Context<'_>,
        #[graphql(
            desc = "A number (`15`), an inclusive range (`1-50`), a code prefix (`P`), or free \
                    text matched against the titles. Omit for the whole index."
        )]
        filter: Option<String>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlSongSummary>> {
        ballad_index(
            ctx,
            IndexKind::Laws,
            filter,
            PageArgs {
                after,
                before,
                first,
                last,
            },
        )
        .await
    }

    /// One artist's index page. One request. Their discography, biography and
    /// songs hang off it, each a further request when selected.
    async fn artist(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Path to the artist's index, e.g. /martin.carthy/")] path: String,
    ) -> Result<Option<GqlArtist>> {
        load_artist(ctx, &path).await
    }

    /// Search releases by artist or album name, through the archive's own
    /// endpoint. One request. Results come grouped by credited artist, as the
    /// archive groups them.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn records(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Artist or album name, or part of one.")] query: String,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlArtistRecords>> {
        if query.trim().is_empty() {
            return Err(async_graphql::Error::new(
                "A record search needs an artist or album name.",
            ));
        }

        let found = ctx
            .data::<RecordSearches>()?
            .load_one(query)
            .await
            .map_err(to_gql_error)?
            .unwrap_or_default();

        let groups: Vec<GqlArtistRecords> = found
            .iter()
            .map(|(artist, albums)| GqlArtistRecords {
                artist: artist.clone(),
                albums: albums.iter().cloned().map(GqlAlbum::from).collect(),
            })
            .collect();

        paginate(
            groups,
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |g| g.artist.clone(),
        )
    }

    /// One release and its tracklist. One request.
    ///
    /// `path` may carry a `#fragment`, which selects one release from a page
    /// documenting several. `tracks` is free here — this fetched the page.
    async fn album(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Album page path, fragment included where a page holds several.")]
        path: String,
    ) -> Result<Option<GqlAlbum>> {
        load_album(ctx, &path).await
    }

    /// The record labels with a discography on the archive, read from its
    /// navigation. One request; `totalCount` is then free.
    ///
    /// Selecting `albums` on the results is one further request per label.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn labels(
        &self,
        ctx: &Context<'_>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlLabel>> {
        let labels = load_labels(ctx).await?;
        paginate(
            labels.iter().cloned().map(GqlLabel::from).collect(),
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |l| l.0.path.clone(),
        )
    }

    /// The archive's bibliography — the books its song pages cite, with
    /// publishers, years, and links to full texts online where they exist. One
    /// request; `totalCount` is then free.
    ///
    /// The same list `BookRef.book` resolves against, so asking here and
    /// following citations there costs one page between them.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn books(
        &self,
        ctx: &Context<'_>,
        #[graphql(
            desc = "Keep only books in this section, matched case-insensitively — \
                    \"Ballads and Songs\", \"Folk Song and Music\", \"Biographies\", \
                    \"Other Books\". Omit for the whole bibliography."
        )]
        section: Option<String>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlBook>> {
        let section = non_empty(section).map(|s| s.to_lowercase());
        let books: Vec<GqlBook> = load_books(ctx)
            .await?
            .iter()
            .filter(|book| match &section {
                Some(wanted) => book
                    .section
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase().contains(wanted)),
                None => true,
            })
            .cloned()
            .map(GqlBook::from)
            .collect();

        paginate(
            books,
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |b| b.0.id.clone(),
        )
    }

    /// Canal and inland-waterways songs from waterwaysongs.info. One request for
    /// the site's menu, then filtered here.
    ///
    /// The menu carries titles and paths only — every other field is empty until
    /// you fetch the song's own page with `waterwaysSong(path:)`.
    #[graphql(complexity = "page_complexity(first, last, child_complexity)")]
    async fn waterways_songs(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Title or part of one, case-insensitive. Omit to list everything.")]
        query: Option<String>,
        after: Option<String>,
        before: Option<String>,
        first: Option<i32>,
        last: Option<i32>,
    ) -> Result<ListConnection<GqlWaterwaysSong>> {
        let needle = non_empty(query).map(|q| q.to_lowercase());
        let songs: Vec<GqlWaterwaysSong> = load_waterways_index(ctx)
            .await?
            .into_iter()
            .filter(|s| match &needle {
                Some(needle) => s.title.to_lowercase().contains(needle),
                None => true,
            })
            .map(GqlWaterwaysSong::from)
            .collect();

        paginate(
            songs,
            PageArgs {
                after,
                before,
                first,
                last,
            },
            |s| s.path.clone(),
        )
    }

    /// One canal song page in full — lyrics, notes, who recorded it. One request.
    async fn waterways_song(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Path on waterwaysongs.info, e.g. /Songs/H/hard_working.htm")]
        path: String,
    ) -> Result<Option<GqlWaterwaysSong>> {
        load_waterways_song(ctx, &path).await
    }

    /// Any page on either archive, as title, plain text and links. One request.
    ///
    /// The escape hatch for pages with no richer type — essays, book entries,
    /// the site's own commentary. Paths outside the two archives are refused.
    async fn page(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Path, or a full URL on one of the two archives.")] path: String,
        #[graphql(
            desc = "Which archive to resolve `path` against. Defaults to mainlynorfolk.info."
        )]
        source: Option<GqlSource>,
    ) -> Result<Option<GqlPage>> {
        let source = source.map_or(Source::MainlyNorfolk, Into::into);
        load_page(ctx, source, &path).await
    }
}

/// A trimmed argument, or `None` where it was blank. A blank string is not a
/// narrowing, and sending it as one turns a search into a request for the whole
/// index.
fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// How a `filter` on the Child or Laws index is read.
#[derive(Debug, PartialEq, Eq)]
enum BalladFilter {
    /// A bare number: `84`, matched against the numeric part of a code.
    Number(u32),
    /// An inclusive range: `1-50`.
    Range(u32, u32),
    /// A letter prefix: `P` for the Laws P series.
    Prefix(String),
    /// Anything else, matched case-insensitively against the titles.
    Text(String),
}

fn parse_ballad_filter(raw: &str) -> BalladFilter {
    let filter = raw.trim();

    if let Ok(number) = filter.parse::<u32>() {
        return BalladFilter::Number(number);
    }
    if let Some((low, high)) = filter.split_once('-')
        && let (Ok(low), Ok(high)) = (low.trim().parse::<u32>(), high.trim().parse::<u32>())
    {
        return BalladFilter::Range(low.min(high), low.max(high));
    }
    // Laws codes are one or two letters and a number; a bare letter or two is
    // therefore a series, not a title anybody is searching for.
    if !filter.is_empty() && filter.len() <= 2 && filter.chars().all(|c| c.is_ascii_alphabetic()) {
        return BalladFilter::Prefix(filter.to_uppercase());
    }
    BalladFilter::Text(filter.to_lowercase())
}

/// The numeric part of a reference code: `"84"` → 84, `"P15"` → 15.
fn code_number(code: &str) -> Option<u32> {
    code.chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

fn ballad_matches(filter: &BalladFilter, codes: &[String], summary: &SongSummary) -> bool {
    match filter {
        BalladFilter::Number(wanted) => codes.iter().any(|c| code_number(c) == Some(*wanted)),
        BalladFilter::Range(low, high) => codes
            .iter()
            .any(|c| code_number(c).is_some_and(|n| n >= *low && n <= *high)),
        BalladFilter::Prefix(prefix) => codes.iter().any(|c| c.to_uppercase().starts_with(prefix)),
        BalladFilter::Text(text) => {
            summary.title.to_lowercase().contains(text)
                || summary
                    .titles
                    .iter()
                    .any(|t| t.to_lowercase().contains(text))
        }
    }
}

/// Load one of the canon indexes and filter it in memory. The whole list is one
/// request, so filtering here costs nothing beyond it.
async fn ballad_index(
    ctx: &Context<'_>,
    kind: IndexKind,
    filter: Option<String>,
    args: PageArgs,
) -> Result<ListConnection<GqlSongSummary>> {
    let index = load_song_index(ctx, kind).await?;
    let filter = non_empty(filter).map(|f| parse_ballad_filter(&f));

    let codes = |refs: &SongRefs| match kind {
        IndexKind::Laws => refs.laws.clone(),
        _ => refs.child.clone(),
    };
    let songs: Vec<GqlSongSummary> = index
        .iter()
        .filter(|s| {
            filter
                .as_ref()
                .is_none_or(|f| ballad_matches(f, &codes(&s.refs), s))
        })
        .cloned()
        .map(GqlSongSummary::from)
        .collect();

    paginate(songs, args, |s| s.0.path.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(title: &str, child: &[&str], laws: &[&str]) -> SongSummary {
        SongSummary {
            path: format!("/folk/songs/{title}.html"),
            title: title.into(),
            titles: vec![title.into()],
            refs: SongRefs {
                child: child.iter().map(|s| s.to_string()).collect(),
                laws: laws.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
            source: Source::MainlyNorfolk,
        }
    }

    #[test]
    fn a_bare_number_is_a_reference_not_a_title() {
        assert_eq!(parse_ballad_filter("84"), BalladFilter::Number(84));
        assert_eq!(parse_ballad_filter(" 84 "), BalladFilter::Number(84));
    }

    #[test]
    fn ranges_are_inclusive_and_order_insensitive() {
        assert_eq!(parse_ballad_filter("1-50"), BalladFilter::Range(1, 50));
        assert_eq!(parse_ballad_filter("50-1"), BalladFilter::Range(1, 50));

        let filter = parse_ballad_filter("1-50");
        assert!(ballad_matches(
            &filter,
            &["1".into()],
            &summary("x", &[], &[])
        ));
        assert!(ballad_matches(
            &filter,
            &["50".into()],
            &summary("x", &[], &[])
        ));
        assert!(!ballad_matches(
            &filter,
            &["51".into()],
            &summary("x", &[], &[])
        ));
    }

    #[test]
    fn a_letter_is_a_laws_series() {
        assert_eq!(parse_ballad_filter("P"), BalladFilter::Prefix("P".into()));
        assert_eq!(parse_ballad_filter("p"), BalladFilter::Prefix("P".into()));

        let filter = parse_ballad_filter("P");
        let song = summary("The Rambling Boy", &[], &["P15"]);
        assert!(ballad_matches(&filter, &song.refs.laws, &song));

        let other = summary("Young Beichan", &[], &["M20"]);
        assert!(!ballad_matches(&filter, &other.refs.laws, &other));
    }

    #[test]
    fn a_number_matches_the_numeric_part_of_a_code() {
        // Laws writes P15, so `15` has to reach it without the caller knowing
        // which letter the series uses.
        let song = summary("The Rambling Boy", &[], &["P15"]);
        assert!(ballad_matches(
            &BalladFilter::Number(15),
            &song.refs.laws,
            &song
        ));
        assert!(!ballad_matches(
            &BalladFilter::Number(1),
            &song.refs.laws,
            &song
        ));
    }

    #[test]
    fn anything_else_matches_the_titles() {
        let filter = parse_ballad_filter("elfin");
        assert_eq!(filter, BalladFilter::Text("elfin".into()));

        let song = summary("The Elfin Knight", &["2"], &[]);
        assert!(ballad_matches(&filter, &song.refs.child, &song));
        // Case-insensitive, and it looks at the alternate titles too.
        let mut alternates = summary("Whittingham Fair", &["2"], &[]);
        alternates.titles.push("Scarborough Fair".into());
        assert!(ballad_matches(
            &parse_ballad_filter("SCARBOROUGH"),
            &alternates.refs.child,
            &alternates
        ));
    }

    #[test]
    fn a_blank_argument_is_not_a_narrowing() {
        assert_eq!(non_empty(Some("   ".into())), None);
        assert_eq!(non_empty(Some(" carthy ".into())), Some("carthy".into()));
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn codes_without_digits_never_match_a_number() {
        assert_eq!(code_number("P15"), Some(15));
        assert_eq!(code_number("84"), Some(84));
        assert_eq!(code_number("trad"), None);
    }
}
