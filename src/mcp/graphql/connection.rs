//! Relay-style connections over the archive's lists.
//!
//! **Paging is slicing.** The archive publishes whole pages, not windows: the
//! Child index is one document listing every Child ballad, a discography is one
//! document listing every release, `search.php` answers with the entire result
//! set. There is no offset, no continuation token, nothing to ask for less. So
//! the fetch is what it is, and `first`/`after` decide what of it gets
//! serialised. Two honest consequences, both said in the schema rather than
//! implied away:
//!
//! - `totalCount` is **free**. It is the length of a list already in hand, so
//!   selecting it costs nothing and it is always exact.
//! - A cursor is the item's `path` — stable when neighbouring entries change,
//!   and legible to a model composing the next page, since it is an id it has
//!   already seen. Lists whose items have no path of their own (a song's
//!   recordings, sentences parsed out of prose) use their position instead,
//!   which is stable because the list arrives whole and in document order. If
//!   the archive edits the page out from under a cursor you get a "restart
//!   pagination" error rather than a quietly different page.

use async_graphql::connection::{Connection, Edge};
use async_graphql::{OutputType, Result, SimpleObject};

/// Page size used when a connection names no `first`/`last`.
pub const DEFAULT_PAGE: u32 = 25;
/// Hard cap on a single page, and the figure nested lists are costed at.
pub const MAX_PAGE: u32 = 100;

pub fn clamp_page(size: Option<u32>) -> u32 {
    size.unwrap_or(DEFAULT_PAGE).min(MAX_PAGE)
}

/// Arguments every connection accepts.
pub struct PageArgs {
    pub after: Option<String>,
    pub before: Option<String>,
    pub first: Option<i32>,
    pub last: Option<i32>,
}

/// The extra field on a connection built from a list already in hand.
#[derive(SimpleObject)]
pub struct CountFields {
    /// How many items match, before paging. Free to ask for — this list arrived
    /// whole from one page, so the count is a length.
    pub total_count: u64,
}

/// A connection over an in-memory list. The GraphQL type name comes from the
/// node, so `SongSummary` gives `SongSummaryConnection`.
pub type ListConnection<T> = Connection<String, T, CountFields>;

/// Where a page sits in a list, with its items already paired with cursors.
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<(String, T)>,
    pub start: usize,
    pub total: usize,
    pub has_previous: bool,
    pub has_next: bool,
}

/// Work out which slice of a list a page refers to.
pub fn page_of<T>(
    items: Vec<T>,
    args: PageArgs,
    cursor_of: impl Fn(&T) -> String,
) -> Result<Page<T>> {
    if args.first.is_some() && args.last.is_some() {
        return Err(async_graphql::Error::new(
            "Pass `first` or `last`, not both.",
        ));
    }
    if args.after.is_some() && args.before.is_some() {
        return Err(async_graphql::Error::new(
            "Pass `after` or `before`, not both.",
        ));
    }

    let cursors: Vec<String> = items.iter().map(&cursor_of).collect();
    let total = items.len();

    let locate = |cursor: &str| {
        cursors.iter().position(|c| c == cursor).ok_or_else(|| {
            async_graphql::Error::new(format!(
                "Cursor {cursor:?} is no longer in this list — the archive page it came from \
                 has changed, or a different filter is in play, since the cursor was issued. \
                 Restart pagination without `after`/`before`."
            ))
        })
    };

    let mut start = match &args.after {
        Some(cursor) => locate(cursor)? + 1,
        None => 0,
    };
    let mut end = match &args.before {
        Some(cursor) => locate(cursor)?,
        None => total,
    };
    end = end.max(start);

    match (args.first, args.last) {
        (Some(first), _) => end = end.min(start + clamp_page(Some(first.max(0) as u32)) as usize),
        (_, Some(last)) => {
            start = end.saturating_sub(clamp_page(Some(last.max(0) as u32)) as usize)
        }
        _ => end = end.min(start + clamp_page(None) as usize),
    }

    Ok(Page {
        items: items
            .into_iter()
            .zip(cursors)
            .skip(start)
            .take(end - start)
            .map(|(node, cursor)| (cursor, node))
            .collect(),
        start,
        total,
        has_previous: start > 0,
        has_next: end < total,
    })
}

/// Paginate a list already in hand.
pub fn paginate<T: OutputType>(
    items: Vec<T>,
    args: PageArgs,
    cursor_of: impl Fn(&T) -> String,
) -> Result<ListConnection<T>> {
    let page = page_of(items, args, cursor_of)?;
    let mut connection = Connection::with_additional_fields(
        page.has_previous,
        page.has_next,
        CountFields {
            total_count: page.total as u64,
        },
    );
    connection.edges.extend(
        page.items
            .into_iter()
            .map(|(cursor, node)| Edge::new(cursor, node)),
    );
    Ok(connection)
}

/// Cost of a connection field: the page size asked for, times the cost of a node.
pub fn page_complexity(first: Option<i32>, last: Option<i32>, child_complexity: usize) -> usize {
    let requested = first.or(last).map(|n| n.max(0) as u32);
    clamp_page(requested) as usize * child_complexity
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(items: &[&str], args: PageArgs) -> Result<Page<String>> {
        page_of(
            items.iter().map(|s| s.to_string()).collect(),
            args,
            Clone::clone,
        )
    }

    fn args(after: Option<&str>, first: Option<i32>) -> PageArgs {
        PageArgs {
            after: after.map(str::to_string),
            before: None,
            first,
            last: None,
        }
    }

    #[test]
    fn a_cursor_resumes_after_the_item_it_names() {
        let p = page(&["a", "b", "c", "d"], args(Some("b"), Some(2))).unwrap();
        let got: Vec<_> = p.items.iter().map(|(_, v)| v.as_str()).collect();
        assert_eq!(got, ["c", "d"]);
        assert_eq!(p.total, 4);
        assert!(p.has_previous);
        assert!(!p.has_next);
    }

    #[test]
    fn a_stale_cursor_says_how_to_recover() {
        let err = page(&["a", "b"], args(Some("gone"), None)).unwrap_err();
        assert!(err.message.contains("Restart pagination"), "{err:?}");
    }

    #[test]
    fn first_and_last_together_are_rejected() {
        let err = page(
            &["a"],
            PageArgs {
                after: None,
                before: None,
                first: Some(1),
                last: Some(1),
            },
        )
        .unwrap_err();
        assert!(err.message.contains("not both"));
    }

    #[test]
    fn last_takes_from_the_end() {
        let p = page(
            &["a", "b", "c"],
            PageArgs {
                after: None,
                before: None,
                first: None,
                last: Some(2),
            },
        )
        .unwrap();
        let got: Vec<_> = p.items.iter().map(|(_, v)| v.as_str()).collect();
        assert_eq!(got, ["b", "c"]);
        assert!(p.has_previous);
    }

    #[test]
    fn page_size_is_capped() {
        // The song index runs to thousands of entries, so an unbounded `first`
        // is the difference between a page and the whole archive.
        let items: Vec<String> = (0..500).map(|i| i.to_string()).collect();
        let p = page_of(items, args(None, Some(9999)), Clone::clone).unwrap();
        assert_eq!(p.items.len(), MAX_PAGE as usize);
        assert!(p.has_next);
    }

    #[test]
    fn no_page_size_takes_the_default() {
        let items: Vec<String> = (0..500).map(|i| i.to_string()).collect();
        let p = page_of(items, args(None, None), Clone::clone).unwrap();
        assert_eq!(p.items.len(), DEFAULT_PAGE as usize);
    }

    #[test]
    fn a_page_costs_its_size_times_the_node() {
        assert_eq!(page_complexity(Some(10), None, 3), 30);
        // Unstated page sizes cost the default, not nothing.
        assert_eq!(page_complexity(None, None, 1), DEFAULT_PAGE as usize);
        assert_eq!(page_complexity(Some(9999), None, 1), MAX_PAGE as usize);
    }
}
