use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use mainlynorfolk_mcp::{client::Client, mcp};
use std::io;
use tracing_subscriber::EnvFilter;

/// MCP server for the Mainly Norfolk English folk archive.
///
/// With no arguments it speaks MCP over stdio, which is what a desktop client
/// launches. The flags are the two other ways in: a hosted deployment
/// (`--http`), and a human wanting to look at the schema (`--graphiql`).
#[derive(Parser)]
#[command(name = "folk")]
#[command(version, about, long_about = None)]
struct Cli {
    /// Housekeeping that runs instead of the server.
    #[command(subcommand)]
    command: Option<Command>,

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

    /// Serve the GraphiQL IDE at /, and the /graphql it talks to (implies
    /// --graphql).
    #[arg(long)]
    graphiql: bool,

    /// Open the GraphiQL IDE in your browser once listening (implies
    /// --graphiql).
    #[arg(long)]
    browser: bool,
}

/// The things that are not "run the server".
///
/// Subcommands rather than flags, because each one does something else entirely
/// and exits — and because it is what the sibling tools already use, so
/// `folk completions zsh` is what a hand reaches for.
#[derive(Subcommand)]
enum Command {
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Empty the on-disk page cache.
    ClearCache,
}

/// Which surfaces the flags ask for, and where to bind them.
///
/// `None` for the address means stdio, which is the whole point of the default:
/// a desktop client launches the binary with no arguments and gets an MCP
/// server on stdin/stdout.
///
/// Asking for a surface is asking for what it needs. Opening a browser at the
/// IDE means serving the IDE, which means serving the `/graphql` it talks to —
/// refusing to infer that would leave the user spelling out three flags to mean
/// one thing. `--http` is separate: it is MCP's own transport, and it is the
/// only flag that puts MCP on the listener.
fn resolve(cli: &Cli) -> (Option<String>, mcp::HttpSurfaces) {
    let graphiql = cli.graphiql || cli.browser;
    let graphql = cli.graphql || graphiql;

    let addr = cli
        .http
        .clone()
        .or_else(|| graphql.then(|| mcp::DEFAULT_HTTP_ADDR.to_string()));

    (
        addr,
        mcp::HttpSurfaces {
            mcp: cli.http.is_some(),
            graphql,
            graphiql,
            browser: cli.browser,
        },
    )
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

    match cli.command {
        Some(Command::Completions { shell }) => {
            generate(shell, &mut Cli::command(), "folk", &mut io::stdout());
            return;
        }
        Some(Command::ClearCache) => {
            match Client::new().and_then(|c| c.clear_cache()) {
                Ok(removed) => eprintln!("Cleared {removed} cached pages"),
                Err(e) => {
                    eprintln!("Could not clear the cache: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        None => {}
    }

    let (addr, surfaces) = resolve(&cli);

    let served = match addr {
        Some(addr) => mcp::run_http_server(&addr, surfaces).await,
        None => mcp::run_server().await,
    };

    if let Err(e) = served {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> (Option<String>, mcp::HttpSurfaces) {
        let mut argv = vec!["folk"];
        argv.extend_from_slice(args);
        resolve(&Cli::parse_from(argv))
    }

    #[test]
    fn no_flags_is_stdio_and_nothing_else() {
        let (addr, s) = parse(&[]);
        assert_eq!(addr, None);
        assert!(!s.mcp && !s.graphql && !s.graphiql && !s.browser);
    }

    #[test]
    fn browser_implies_the_ide_and_its_endpoint_but_not_mcp() {
        // `--browser` alone: serve the IDE and the /graphql it talks to, open
        // it, and leave MCP off the listener.
        let (addr, s) = parse(&["--browser"]);
        assert_eq!(addr.as_deref(), Some(mcp::DEFAULT_HTTP_ADDR));
        assert!(s.browser && s.graphiql && s.graphql);
        assert!(!s.mcp, "--browser must not mount MCP over HTTP");
    }

    #[test]
    fn graphiql_with_http_serves_both_and_opens_nothing() {
        let (addr, s) = parse(&["--graphiql", "--http"]);
        assert_eq!(addr.as_deref(), Some(mcp::DEFAULT_HTTP_ADDR));
        assert!(s.graphiql && s.graphql, "--graphiql implies --graphql");
        assert!(s.mcp, "--http mounts MCP");
        assert!(!s.browser, "nothing asked for a browser");
    }

    #[test]
    fn http_alone_is_mcp_only() {
        // The hosted case: a gateway proxies /mcp and wants no browsable
        // surfaces it would then have to think about exposing.
        let (addr, s) = parse(&["--http"]);
        assert_eq!(addr.as_deref(), Some(mcp::DEFAULT_HTTP_ADDR));
        assert!(s.mcp);
        assert!(!s.graphql && !s.graphiql);
    }

    #[test]
    fn an_explicit_address_is_honoured() {
        let (addr, _) = parse(&["--http", "0.0.0.0:9000"]);
        assert_eq!(addr.as_deref(), Some("0.0.0.0:9000"));
    }

    #[test]
    fn graphql_alone_binds_a_listener_without_mcp_or_the_ide() {
        let (addr, s) = parse(&["--graphql"]);
        assert!(
            addr.is_some(),
            "there is nowhere to mount a route over stdio"
        );
        assert!(s.graphql && !s.graphiql && !s.mcp);
    }
}
