//! The Mainly Norfolk folk archive, as a CLI, a GraphQL API and an MCP server.
//!
//! This crate is both a binary (`main.rs`) and a library. The library surface
//! exists so a hosted MCP service can link the archive and GraphQL machinery
//! directly rather than shelling out to the binary.
//!
//! Layering, outermost first:
//!
//! - [`mcp`] — the MCP server and the GraphQL schema over [`archive`].
//! - [`commands`] — one function per CLI subcommand, also over [`archive`].
//! - [`archive`] — typed accessors: "give me this song", "search for that
//!   artist". The only layer that pairs a fetch with a parse.
//! - [`parse`] — pure `&str` → model functions. No I/O, unit-testable against
//!   saved fixtures.
//! - [`client`] — HTTP, the disk cache, and the politeness budget.

pub mod archive;
pub mod client;
pub mod commands;
pub mod error;
pub mod mcp;
pub mod models;
pub mod parse;
