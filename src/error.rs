use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A page fetch came back with a non-success status.
    #[error("Fetch failed: {url} returned {status}")]
    Fetch { url: String, status: u16 },

    /// The archive answered, but the page wasn't the shape the parser expects.
    /// Carries what was being looked for, not the whole document — these end up
    /// in CLI output and GraphQL errors.
    #[error("Could not parse {what} from {url}")]
    Parse { what: &'static str, url: String },

    #[error("Not found: {0}")]
    NotFound(String),

    /// A path that doesn't belong to any known archive. Guards against the
    /// fetcher being pointed at arbitrary hosts by a model-composed query.
    #[error("{0:?} is not a path on any archive this tool knows about")]
    ForeignPath(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Cache error: {0}")]
    Cache(String),
}

pub type Result<T> = std::result::Result<T, Error>;
