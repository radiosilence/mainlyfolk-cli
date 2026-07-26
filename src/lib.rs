//! The Mainly Norfolk folk archive, as a GraphQL API behind an MCP server.
//!
//! This crate is both a binary (`main.rs`) and a library. The library surface
//! exists so a hosted MCP service can link the archive and GraphQL machinery
//! directly rather than shelling out to the binary.
//!
//! There is one path through it, and everything reads follows it: a **GraphQL
//! resolver** asks a **DataLoader**, which makes the **API call**. No resolver
//! fetches anything itself, which is what makes the batching and the
//! request-scoped caching total rather than best-effort.
//!
//! Layering, outermost first:
//!
//! - [`mcp`] — the MCP server and the GraphQL schema. Resolvers and loaders.
//! - [`archive`] — what an API call is: the archive's own `search.php`
//!   endpoints, and the paths everything else is fetched from.
//! - [`parse`] — pure `&str` → model functions. No I/O, unit-testable against
//!   saved fixtures.
//! - [`client`] — HTTP, the caches, and the politeness budget.

pub mod archive;
pub mod client;
pub mod error;
pub mod mcp;
pub mod models;
pub mod parse;
