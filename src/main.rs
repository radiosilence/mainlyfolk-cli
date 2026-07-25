use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use mainlynorfolk_cli::archive::RefKey;
use mainlynorfolk_cli::models::Output;
use mainlynorfolk_cli::{commands, mcp};
use std::io;
use tracing_subscriber::EnvFilter;

/// Flags every command that reads the archive accepts.
#[derive(Args, Debug, Clone, Default)]
struct CacheOpts {
    /// Bypass the on-disk page cache for this run.
    #[arg(long, global = true)]
    no_cache: bool,
}

/// The reference schemes the archive's own search indexes.
#[derive(clap::ValueEnum, Debug, Clone, Copy)]
enum RefScheme {
    /// Steve Roud's Folk Song Index
    Roud,
    /// Francis James Child's ballad numbers
    Child,
    /// G. Malcolm Laws' American balladry codes
    Laws,
    /// The Greig-Duncan Folk Song Collection
    Gd,
    /// Sam Henry's Songs of the People
    Henry,
}

impl From<RefScheme> for RefKey {
    fn from(s: RefScheme) -> Self {
        match s {
            RefScheme::Roud => RefKey::Roud,
            RefScheme::Child => RefKey::Child,
            RefScheme::Laws => RefKey::Laws,
            RefScheme::Gd => RefKey::GreigDuncan,
            RefScheme::Henry => RefKey::Henry,
        }
    }
}

#[derive(Parser)]
#[command(name = "folk")]
#[command(version, about = "The Mainly Norfolk English folk archive, from the command line", long_about = None)]
struct Cli {
    #[command(flatten)]
    cache: CacheOpts,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search songs by title, author, or reference number
    Search {
        /// Title or part of one. A trailing `*` is a wildcard.
        query: Option<String>,

        /// Narrow to songs associated with this artist
        #[arg(short, long)]
        author: Option<String>,

        /// Reference scheme for --number
        #[arg(long, value_enum, requires = "number")]
        scheme: Option<RefScheme>,

        /// Reference number, e.g. 12 for Roud 12, P15 for Laws P15
        #[arg(short, long)]
        number: Option<String>,
    },

    /// Read a song page — titles, references, lyrics, notes, recordings
    Song {
        /// Song page path, e.g. /martin.carthy/songs/theelfinknight.html
        path: String,

        /// Print only the lyrics
        #[arg(long)]
        lyrics: bool,
    },

    /// Browse the Child Ballads the archive covers — 209 of Child's 305
    Child {
        /// A number (`84`), a range (`1-50`), or text to match
        filter: Option<String>,
    },

    /// Browse the Laws Index — American ballads of British origin
    Laws {
        /// A code (`P15`), a letter prefix (`P`), or text to match
        filter: Option<String>,
    },

    /// Look up an artist and their chronological discography
    Artist {
        /// Artist name or path, e.g. "Martin Carthy" or /martin.carthy/
        name: String,
    },

    /// Search releases by artist or album name
    Records {
        /// Artist or album name, or part of one
        query: String,
    },

    /// Read an album page and its tracklist
    Album {
        /// Album page path, fragment included where a page holds several
        path: String,
    },

    /// List the record labels with a discography on the archive
    Labels,

    /// Browse the bibliography — the books the archive's song pages cite
    Books {
        /// Filter by section, author or title
        filter: Option<String>,
    },

    /// Canal and inland-waterways songs, from waterwaysongs.info
    Waterways {
        /// Title or part of one. Omit to list everything.
        query: Option<String>,
    },

    /// Read any archive page as plain text
    Page {
        /// Path or full URL on either archive
        path: String,
    },

    /// Show what the archive changed recently
    Latest,

    /// Inspect or empty the on-disk page cache
    Cache {
        /// Delete every cached page
        #[arg(long)]
        clear: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Run as MCP (Model Context Protocol) server for Claude integration
    Mcp {
        /// Serve MCP over streamable HTTP at /mcp instead of stdio, on this
        /// address (default 127.0.0.1:8080).
        #[arg(
            long,
            value_name = "ADDR",
            num_args = 0..=1,
            default_missing_value = mcp::DEFAULT_HTTP_ADDR,
        )]
        http: Option<String>,

        /// Serve plain GraphQL-over-HTTP at /graphql
        #[arg(long)]
        graphql: bool,

        /// Serve the GraphiQL IDE at /, and the /graphql it talks to
        #[arg(long)]
        graphiql: bool,

        /// Open the GraphiQL IDE in your browser once listening
        #[arg(long, requires = "graphiql")]
        browser: bool,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();
    let no_cache = cli.cache.no_cache;

    let result = match cli.command {
        Commands::Search {
            query,
            author,
            scheme,
            number,
        } => {
            commands::search(
                no_cache,
                query.as_deref(),
                author.as_deref(),
                scheme.map(Into::into),
                number.as_deref(),
            )
            .await
        }

        Commands::Song { path, lyrics } => commands::song(no_cache, &path, lyrics).await,

        Commands::Child { filter } => commands::child(no_cache, filter.as_deref()).await,

        Commands::Laws { filter } => commands::laws(no_cache, filter.as_deref()).await,

        Commands::Artist { name } => commands::artist(no_cache, &name).await,

        Commands::Records { query } => commands::records(no_cache, &query).await,

        Commands::Album { path } => commands::album(no_cache, &path).await,

        Commands::Labels => commands::labels(no_cache).await,

        Commands::Books { filter } => commands::books(no_cache, filter.as_deref()).await,

        Commands::Waterways { query } => commands::waterways(no_cache, query.as_deref()).await,

        Commands::Page { path } => commands::page(no_cache, &path).await,

        Commands::Latest => commands::latest(no_cache).await,

        Commands::Cache { clear } => commands::cache(clear),

        Commands::Completions { shell } => {
            generate(shell, &mut Cli::command(), "folk", &mut io::stdout());
            return;
        }

        Commands::Mcp {
            http,
            graphql,
            graphiql,
            browser,
        } => {
            // `--http` is MCP's own transport; `--graphql`/`--graphiql` are
            // separate surfaces that happen to need a listener too. Asking for
            // any of them binds one — there is nowhere to mount an HTTP route
            // over stdio — and only `--http` puts MCP on it.
            let addr = http
                .clone()
                .or_else(|| (graphql || graphiql).then(|| mcp::DEFAULT_HTTP_ADDR.to_string()));
            let served = match addr {
                Some(addr) => {
                    mcp::run_http_server(
                        &addr,
                        mcp::HttpSurfaces {
                            mcp: http.is_some(),
                            graphql,
                            graphiql,
                            browser,
                        },
                    )
                    .await
                }
                None => mcp::run_server().await,
            };
            served.map_err(|e| mainlynorfolk_cli::error::Error::Cache(e.to_string()))
        }
    };

    if let Err(e) = result {
        Output::<()>::error(e.to_string()).print();
        std::process::exit(1);
    }
}
