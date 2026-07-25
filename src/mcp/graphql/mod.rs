//! GraphQL schema over the archives.
//!
//! One composable query interface rather than a tool per lookup. The archive's
//! interesting questions are joins — what else is on the album this recording
//! came from, which artists claim this song, what else that label put out — and
//! a graph answers them in one round trip instead of a conversation.
//!
//! It is read-only, so there is no mutation root: these are public websites, and
//! nothing here has anything to write with.

use async_graphql::{EmptyMutation, EmptySubscription, Schema};

pub mod connection;
pub mod loaders;
mod query;
#[cfg(test)]
mod tests;
pub mod types;

use query::QueryRoot;

pub type FolkSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

/// The archive accessor, injected per request. Shared (`Arc`) because every
/// loader holds one; its own methods take `&self`, so no lock is needed.
pub type SharedArchive = std::sync::Arc<crate::archive::Archive>;

/// Maximum selection-set nesting.
///
/// A stop, not a discouragement. Depth is the point of this schema: the archive
/// is a web of cross-references and following them is what it is for. `artist →
/// discography → album → tracks → song → recordings → album → tracks → song` is
/// a reasonable question, and it is nine levels before it even begins to repeat;
/// `song → sameRoud → song → sameRoud` recurses from there for as long as it is
/// useful. The cap exists only because the cycles have no natural end, and an
/// unbounded query would walk the archive forever a page at a time.
const MAX_DEPTH: usize = 30;

/// Build the GraphQL schema. It holds nothing per-caller — the archive and the
/// loaders are supplied per request by [`request`].
pub fn build_schema() -> FolkSchema {
    // Complexity is deliberately **not** capped. Fields still declare costs, and
    // those costs are spelled out in the descriptions so a caller can decide
    // whether an edge is worth the fetches — but they are guidance, not a gate.
    // Refusing an expensive-but-legitimate query leaves the caller guessing at a
    // threshold it cannot see, which is a bad deal for a model composing one.
    // The politeness budget lives where it can actually be enforced: the
    // client's concurrency cap.
    //
    // Depth is capped generously rather than tightly — see [`MAX_DEPTH`].
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .limit_depth(MAX_DEPTH)
        .finish()
}

/// Build a GraphQL request carrying the archive and a fresh set of DataLoaders.
///
/// Loaders are per request on purpose — their cache is then a request-scoped
/// cache, so repeating a page inside one query is free while nothing is retained
/// long enough to go stale. Across requests, the client's disk cache is what
/// saves the archive the traffic.
pub fn request(query: &str, archive: SharedArchive) -> async_graphql::Request {
    let loaders = loaders::Loaders::new(archive);
    async_graphql::Request::new(query)
        .data(loaders.pages)
        .data(loaders.songs)
        .data(loaders.albums)
        .data(loaders.artists)
        .data(loaders.discographies)
        .data(loaders.waterways)
        .data(loaders.indexes)
        .data(loaders.searches)
        .data(loaders.record_searches)
}
