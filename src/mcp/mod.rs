//! MCP server over the archive. Owned by the GraphQL/MCP work.

pub mod graphql;

/// Where the HTTP server listens when no address is given.
pub const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:8080";

/// Which surfaces [`run_http_server`] mounts. Each is independent: MCP's
/// streamable-HTTP transport and a browsable GraphQL endpoint are different
/// things that happen to share a port.
#[derive(Clone, Copy)]
pub struct HttpSurfaces {
    /// MCP streamable-HTTP at `/mcp`.
    pub mcp: bool,
    /// Plain GraphQL-over-HTTP at `/graphql`.
    pub graphql: bool,
    /// The GraphiQL IDE at `/`. Implies `graphql` — it is the IDE's endpoint.
    pub graphiql: bool,
    /// Open the IDE in the default browser once listening.
    pub browser: bool,
}

pub async fn run_server() -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}

pub async fn run_http_server(_addr: &str, _surfaces: HttpSurfaces) -> anyhow::Result<()> {
    anyhow::bail!("not yet implemented")
}
