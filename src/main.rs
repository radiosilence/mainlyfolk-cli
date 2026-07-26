use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use mainlynorfolk_mcp::{client::Client, mcp};
use std::io;
use tracing_subscriber::EnvFilter;

/// MCP server for the Mainly Norfolk English folk archive.
///
/// With no flags it speaks MCP over stdio, which is what a desktop client
/// launches. The flags exist for the two other ways in: a hosted deployment
/// (`--http`), and a human wanting to look at the schema (`--graphiql`).
#[derive(Parser)]
#[command(name = "mainlynorfolk-mcp")]
#[command(version, about, long_about = None)]
struct Cli {
    /// Serve MCP over streamable HTTP at /mcp instead of stdio, on this
    /// address (default 127.0.0.1:8080).
    #[arg(
        long,
        value_name = "ADDR",
        num_args = 0..=1,
        default_missing_value = mcp::DEFAULT_HTTP_ADDR,
    )]
    http: Option<String>,

    /// Serve plain GraphQL-over-HTTP at /graphql, for anything speaking
    /// GraphQL directly rather than through MCP's JSON-RPC envelope.
    #[arg(long)]
    graphql: bool,

    /// Serve the GraphiQL IDE at /, and the /graphql it talks to.
    #[arg(long)]
    graphiql: bool,

    /// Open the GraphiQL IDE in your browser once listening.
    #[arg(long, requires = "graphiql")]
    browser: bool,

    /// Empty the on-disk page cache and exit.
    #[arg(long)]
    clear_cache: bool,

    /// Generate shell completions and exit.
    #[arg(long, value_name = "SHELL", value_enum)]
    completions: Option<Shell>,
}

#[tokio::main]
async fn main() {
    // stderr, always: stdout is the MCP transport over stdio, and a stray log
    // line there is a protocol error rather than a cosmetic one.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        generate(
            shell,
            &mut Cli::command(),
            "mainlynorfolk-mcp",
            &mut io::stdout(),
        );
        return;
    }

    if cli.clear_cache {
        match Client::new().and_then(|c| c.clear_cache()) {
            Ok(removed) => eprintln!("Cleared {removed} cached pages"),
            Err(e) => {
                eprintln!("Could not clear the cache: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // `--http` is MCP's own transport; `--graphql`/`--graphiql` are separate
    // surfaces that happen to need a listener too. Asking for any of them binds
    // one — there is nowhere to mount an HTTP route over stdio — and only
    // `--http` puts MCP on it.
    let addr = cli
        .http
        .clone()
        .or_else(|| (cli.graphql || cli.graphiql).then(|| mcp::DEFAULT_HTTP_ADDR.to_string()));

    let served = match addr {
        Some(addr) => {
            mcp::run_http_server(
                &addr,
                mcp::HttpSurfaces {
                    mcp: cli.http.is_some(),
                    graphql: cli.graphql,
                    graphiql: cli.graphiql,
                    browser: cli.browser,
                },
            )
            .await
        }
        None => mcp::run_server().await,
    };

    if let Err(e) = served {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
