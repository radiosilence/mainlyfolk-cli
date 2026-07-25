//! MCP (Model Context Protocol) server over the folk archives.
//!
//! Exposes the archive through two GraphQL tools:
//! - `folk_schema` — the SDL and the costs that go with it
//! - `folk` — executes a query
//!
//! The schema is several thousand tokens, so it stays behind a tool call rather
//! than riding in the always-loaded tool descriptions: a session that never
//! mentions a folk song should pay close to nothing for having this server
//! connected.
//!
//! There is nothing to authenticate. Both archives are public static sites, so
//! this server has no credentials, no per-caller state, and no writes — one
//! shared [`Archive`] answers every request.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_handler, tool_router,
};

use crate::archive::Archive;
use crate::client::Client;

type ToolResult = std::result::Result<CallToolResult, McpError>;

pub mod graphql;

use graphql::{FolkSchema, SharedArchive};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GraphqlRequest {
    /// The GraphQL query string
    pub query: String,
    /// Optional JSON-encoded variables for the query
    #[serde(default)]
    pub variables: Option<String>,
}

#[derive(Clone)]
pub struct FolkMcp {
    schema: Arc<FolkSchema>,
    archive: SharedArchive,
    #[allow(dead_code)] // referenced by #[tool_handler] macro expansion
    tool_router: ToolRouter<Self>,
}

impl FolkMcp {
    /// One archive for the whole process: the HTTP client, its concurrency
    /// permit and its disk cache are shared, which is what keeps the politeness
    /// budget global rather than per-request.
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            schema: Arc::new(graphql::build_schema()),
            archive: Arc::new(Archive::new(Client::new()?)),
            tool_router: Self::tool_router(),
        })
    }

    fn text_result(text: impl Into<String>) -> ToolResult {
        Ok(CallToolResult::success(vec![Content::text(text.into())]))
    }

    fn error_result(msg: impl Into<String>) -> ToolResult {
        Ok(CallToolResult::error(vec![Content::text(msg.into())]))
    }
}

#[tool_router]
impl FolkMcp {
    #[tool(
        name = "folk_schema",
        title = "Folk Archive Schema",
        description = "The folk archive's GraphQL schema. Call once before the first `folk` query."
    )]
    async fn folk_schema(&self) -> ToolResult {
        Self::text_result(self.schema.sdl())
    }

    #[tool(
        name = "folk",
        title = "Folk Archive",
        description = "Execute a GraphQL query against the folk archive: songs, lyrics, Child and Laws ballads, artists, discographies, albums, labels, and canal songs. Get the schema from `folk_schema` first. Variables are a JSON string."
    )]
    async fn folk(&self, Parameters(req): Parameters<GraphqlRequest>) -> ToolResult {
        let mut request = graphql::request(&req.query, self.archive.clone());

        if let Some(ref vars) = req.variables {
            match serde_json::from_str::<serde_json::Value>(vars) {
                Ok(serde_json::Value::Object(map)) => {
                    request = request.variables(async_graphql::Variables::from_json(
                        serde_json::Value::Object(map),
                    ));
                }
                Ok(_) => return Self::error_result("Variables must be a JSON object"),
                Err(e) => return Self::error_result(format!("Invalid variables JSON: {e}")),
            }
        }

        let response = self.schema.execute(request).await;
        let json = serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("{{\"error\": \"Serialization failed: {e}\"}}"));

        Self::text_result(json)
    }
}

#[tool_handler]
impl ServerHandler for FolkMcp {
    fn get_info(&self) -> ServerInfo {
        let server_info = Implementation::new("folk", env!("CARGO_PKG_VERSION"))
            .with_title("Folk Archive MCP Server")
            .with_website_url("https://github.com/radiosilence/mainlyfolk-cli");

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            // Only the fallback for a client asking for a version the SDK
            // doesn't know; anything known is echoed back during negotiation.
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_server_info(server_info)
            // Deliberately terse: instructions and tool descriptions are loaded
            // into every session, most of which never touch folk music. The
            // schema and the costs that go with it are a tool call away.
            .with_instructions(
                "A read-only archive of English folk music — mainlynorfolk.info and \
                 waterwaysongs.info — as a small GraphQL API. Read the schema once with \
                 `folk_schema`, then query with `folk`. These are hand-maintained volunteer \
                 sites with no API: every edge you select is a page fetched from them, and \
                 each field's description says what it costs. Ask for what you need rather \
                 than walking the graph to see what is there.",
            )
    }
}

/// Run the MCP server over stdio.
pub async fn run_server() -> anyhow::Result<()> {
    use rmcp::{ServiceExt, transport::stdio};

    let service = FolkMcp::new()?;
    let server = service
        .serve(stdio())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start MCP server: {}", e))?;

    server
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {}", e))?;

    Ok(())
}

/// Body of a GraphQL-over-HTTP request, as GraphiQL sends it.
#[derive(serde::Deserialize)]
struct HttpGraphqlRequest {
    query: String,
    #[serde(default)]
    variables: Option<serde_json::Value>,
    #[serde(default, rename = "operationName")]
    operation_name: Option<String>,
}

/// The GraphiQL IDE page — see `templates/graphiql.html`.
#[derive(askama::Template)]
#[template(path = "graphiql.html")]
struct GraphiqlPage<'a> {
    title: &'a str,
    endpoint: &'a str,
}

/// Whether every top-level selection is an introspection field, and so can be
/// answered from the schema alone.
///
/// GraphiQL sends exactly this on load to build its docs, autocomplete and
/// explorer. Answering it from the schema means opening the IDE touches neither
/// archive — reading the docs for a site should not cost that site a request.
/// Anything unparseable, or that mixes in real fields, is not introspection.
fn is_introspection_only(query: &str) -> bool {
    use async_graphql::parser::types::Selection;

    let Ok(doc) = async_graphql::parser::parse_query(query) else {
        return false;
    };
    doc.operations.iter().all(|(_, op)| {
        op.node
            .selection_set
            .node
            .items
            .iter()
            .all(|item| match &item.node {
                Selection::Field(field) => field.node.name.node.starts_with("__"),
                // Fragments could hide anything; treat them as real fields.
                _ => false,
            })
    })
}

/// Plain GraphQL-over-HTTP, for browsers and anything else that speaks it
/// directly rather than through MCP's JSON-RPC envelope. Shares the server's
/// schema and archive with the `folk` tool.
async fn graphql_endpoint(
    axum::extract::State(mcp): axum::extract::State<FolkMcp>,
    axum::Json(req): axum::Json<HttpGraphqlRequest>,
) -> axum::Json<async_graphql::Response> {
    let mut request = if is_introspection_only(&req.query) {
        async_graphql::Request::new(&req.query)
    } else {
        graphql::request(&req.query, mcp.archive.clone())
    };
    if let Some(vars) = req.variables {
        request = request.variables(async_graphql::Variables::from_json(vars));
    }
    if let Some(name) = req.operation_name {
        request = request.operation_name(name);
    }
    axum::Json(mcp.schema.execute(request).await)
}

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

/// Run the HTTP server on `addr`, mounting whichever of [`HttpSurfaces`] is
/// enabled.
///
/// There is nothing to authenticate here — the archives are public and this
/// server only reads them. What it does have is a shared politeness budget, so
/// exposing it widely means strangers spending a small website's bandwidth
/// under this tool's user agent. Bind it to localhost unless you mean otherwise.
pub async fn run_http_server(addr: &str, surfaces: HttpSurfaces) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };

    // One shared instance (shared schema, archive, client cache) cloned into
    // each session and used as the axum state for the GraphQL routes.
    let mcp = FolkMcp::new()?;
    let mut router = axum::Router::new();

    if surfaces.mcp {
        // Disable rmcp's DNS-rebinding Host allowlist: this transport is designed
        // to run behind a trusted reverse proxy, which forwards an internal Host
        // (e.g. the service name) that the default allowlist
        // (localhost/127.0.0.1/::1) would reject with 403. Rebinding protection
        // guards browsers hitting a localhost MCP directly — irrelevant for a
        // proxied, non-browser-facing backend; the proxy is the security
        // boundary.
        let config = StreamableHttpServerConfig::default().disable_allowed_hosts();
        let service = StreamableHttpService::new(
            {
                let template = mcp.clone();
                move || Ok(template.clone())
            },
            Arc::new(LocalSessionManager::default()),
            config,
        );
        router = router.nest_service("/mcp", service);
        tracing::info!("MCP streamable-HTTP listening on http://{addr}/mcp");
    }

    if surfaces.graphql || surfaces.graphiql {
        router = router.route("/graphql", axum::routing::post(graphql_endpoint));
        tracing::info!("GraphQL endpoint on http://{addr}/graphql");
    }

    if surfaces.graphiql {
        // Rendered once: nothing in the page varies per request, and a template
        // error should stop the server rather than 500 on every hit.
        let ide = askama::Template::render(&GraphiqlPage {
            title: "Folk Archive GraphQL",
            endpoint: "/graphql",
        })?;
        router = router.route(
            "/",
            axum::routing::get(move || {
                let ide = ide.clone();
                async move { axum::response::Html(ide) }
            }),
        );
        tracing::info!("GraphiQL IDE on http://{addr}/");
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Only once the listener is bound, so the browser cannot beat us to it.
    if surfaces.browser {
        let url = format!("http://{addr}/");
        if let Err(e) = open::that_detached(&url) {
            tracing::warn!("Could not open a browser at {url}: {e}");
        }
    }

    axum::serve(listener, router.with_state(mcp))
        .await
        .map_err(|e| anyhow::anyhow!("MCP HTTP server error: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn introspection_is_answered_from_the_schema() {
        // What GraphiQL sends on load, plus the shapes around it. None of it
        // should reach the archive.
        assert!(is_introspection_only("{ __schema { queryType { name } } }"));
        assert!(is_introspection_only(
            "query IntrospectionQuery { __schema { types { name } } }"
        ));
        assert!(is_introspection_only("{ __type(name: \"Song\") { name } }"));
        assert!(is_introspection_only("{ __typename }"));
    }

    #[test]
    fn real_fields_still_go_to_the_archive() {
        assert!(!is_introspection_only(
            "{ childBallads { nodes { title } } }"
        ));
        // Mixed with introspection, and nested below it, still count as real.
        assert!(!is_introspection_only(
            "{ __typename childBallads { nodes { title } } }"
        ));
        // Fragments could hide anything, and unparseable input proves nothing.
        assert!(!is_introspection_only(
            "{ ...F } fragment F on Query { __typename }"
        ));
        assert!(!is_introspection_only("{ this is not graphql"));
    }
}
