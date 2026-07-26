//! Fetching, caching and politeness.
//!
//! The archives are static sites maintained by hand — mainlynorfolk.info by one
//! person, for decades. There is no API and no rate limit published, which
//! means restraint is ours to impose rather than theirs to enforce. Three
//! things do that, and they are the reason this module exists rather than
//! resolvers calling `reqwest` directly:
//!
//! - **A disk cache.** Pages are keyed by URL under the user's cache directory
//!   and survive process exit, so a session that looks at the Child index ten
//!   times over a week fetches it once.
//! - **Conditional revalidation.** Past the freshness window we re-ask with
//!   `If-None-Match`/`If-Modified-Since`. The archive answers `304` and sends no
//!   body, which is the cheapest thing we can ask of it.
//! - **A concurrency cap.** The GraphQL layer can fan a single query into
//!   hundreds of page loads. [`MAX_CONCURRENT`] is what stands between that and
//!   a small server having a bad afternoon.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::error::{Error, Result};
use crate::models::Source;

/// Sent on every request. Names the tool and links the repo so an archive
/// maintainer seeing it in their logs can find out what it is and complain to
/// somebody. Anonymous scrapers are how sites end up blocking whole ranges.
pub const USER_AGENT: &str = concat!(
    "mainlynorfolk-mcp/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/radiosilence/mainlynorfolk-mcp)"
);

/// Simultaneous in-flight requests, across every source.
///
/// Low on purpose. A graph query asking for a song's every recording can want
/// 50 album pages at once; issuing those 50-wide at a hand-maintained site is
/// rude, and the latency saved over doing them 4-wide is not worth it.
pub const MAX_CONCURRENT: usize = 4;

/// How long a cached page is served without asking the archive anything.
///
/// The site's own footer dates pages in days, and its content is decades old —
/// a day-stale lyric sheet has never hurt anyone. Past this we revalidate
/// rather than refetch, so the usual cost of expiry is a `304`.
pub const FRESH_FOR: Duration = Duration::from_secs(60 * 60 * 24);

/// Request timeout. The archive is a small static host and occasionally slow.
const TIMEOUT: Duration = Duration::from_secs(30);

/// A cached response: the body plus whatever lets us revalidate it cheaply.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    url: String,
    body: String,
    etag: Option<String>,
    last_modified: Option<String>,
    /// When this was last confirmed current — set on a fresh `200` *and*
    /// refreshed on a `304`, so a page that never changes stops being
    /// revalidated every day once it has been confirmed once.
    fetched_at: SystemTime,
}

impl CacheEntry {
    fn is_fresh(&self) -> bool {
        SystemTime::now()
            .duration_since(self.fetched_at)
            .map(|age| age < FRESH_FOR)
            .unwrap_or(false)
    }
}

/// Where cached pages live. `None` disables persistence — the client still
/// works, it just pays for every page every run.
fn cache_dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("mainlynorfolk");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Cache filename for a URL. Hashed because URLs contain `/`, `?` and `#`, and
/// because POST search bodies are folded into the key by the caller.
fn cache_key(url: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}.json", Sha256::digest(url.as_bytes()))
}

/// Pages held in memory before the disk cache is consulted.
///
/// Sized for a long-running MCP server rather than a CLI invocation. A deep
/// graph query walks the same handful of hub pages over and over — the Child
/// index, an artist's discography, the album every track on it points back to —
/// and this is what makes the second visit free rather than a deserialise.
/// 512 pages is a few hundred MB at the archive's largest, and far more than
/// any single query touches.
const MEMORY_CACHE_ENTRIES: usize = 512;

/// The process-wide page cache: URL → body, with a rough LRU bound.
///
/// `Arc<String>` because these are handed to many resolvers at once and the
/// bodies are large — the song index alone is 670KB, and cloning it per
/// resolver was the whole cost this avoids.
#[derive(Default)]
struct MemoryCache {
    entries: HashMap<String, Arc<String>>,
    /// Insertion order, used to evict the oldest once [`MEMORY_CACHE_ENTRIES`]
    /// is reached. Not a true LRU — reads don't promote — because the access
    /// pattern here is "fetch once, read many within one query", where
    /// insertion order and recency barely differ and the bookkeeping isn't
    /// worth it.
    order: VecDeque<String>,
}

impl MemoryCache {
    fn get(&self, url: &str) -> Option<Arc<String>> {
        self.entries.get(url).cloned()
    }

    fn insert(&mut self, url: String, body: Arc<String>) {
        if self.entries.insert(url.clone(), body).is_none() {
            self.order.push_back(url);
        }
        while self.order.len() > MEMORY_CACHE_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

/// Fetches pages from the archives, with two caches in front.
///
/// Cloneable and cheap to clone — the HTTP client, semaphore, memory cache and
/// cache directory are all shared. One instance per process is the intent, and
/// the memory cache is the reason: clone it freely, but do not build a second
/// one.
///
/// The layers, cheapest first: memory, then disk, then a conditional request,
/// then a real fetch. The archive is decades-old material that changes a few
/// times a month, so nearly every read past the first should stop at the first
/// layer.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    limiter: Arc<Semaphore>,
    cache: Option<PathBuf>,
    memory: Arc<Mutex<MemoryCache>>,
}

impl Client {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(TIMEOUT)
                .build()?,
            limiter: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            cache: cache_dir(),
            memory: Arc::new(Mutex::new(MemoryCache::default())),
        })
    }

    /// A client that never touches the disk cache. For tests, and for
    /// `--no-cache`. The memory cache stays — it holds nothing across runs, so
    /// it can't serve anything stale, and turning it off would only mean
    /// fetching the same page twice inside one command.
    pub fn uncached() -> Result<Self> {
        Ok(Self {
            cache: None,
            ..Self::new()?
        })
    }

    fn memory_get(&self, url: &str) -> Option<Arc<String>> {
        self.memory.lock().ok()?.get(url)
    }

    fn memory_put(&self, url: &str, body: &Arc<String>) {
        if let Ok(mut memory) = self.memory.lock() {
            memory.insert(url.to_string(), body.clone());
        }
    }

    /// Fetch `path` from `source`, serving from cache when possible.
    ///
    /// `path` may be site-absolute (`/folk/songs/`) or a full URL belonging to a
    /// known [`Source`]; anything else is [`Error::ForeignPath`]. That check is
    /// the reason resolvers hand paths to this rather than building URLs
    /// themselves: a model composing a GraphQL query must not be able to turn
    /// this tool into an open proxy.
    pub async fn get(&self, source: Source, path: &str) -> Result<Arc<String>> {
        let url = absolute_url(source, path)?;

        if let Some(body) = self.memory_get(&url) {
            tracing::debug!(%url, "memory hit");
            return Ok(body);
        }

        let cached = self.read_cache(&url);
        if let Some(entry) = &cached
            && entry.is_fresh()
        {
            tracing::debug!(%url, "disk hit");
            let body = Arc::new(entry.body.clone());
            self.memory_put(&url, &body);
            return Ok(body);
        }

        let _permit = self
            .limiter
            .acquire()
            .await
            .map_err(|e| Error::Cache(e.to_string()))?;

        let mut request = self.http.get(&url);
        if let Some(entry) = &cached {
            if let Some(etag) = &entry.etag {
                request = request.header(http::header::IF_NONE_MATCH, etag);
            }
            if let Some(modified) = &entry.last_modified {
                request = request.header(http::header::IF_MODIFIED_SINCE, modified);
            }
        }

        let response = request.send().await?;
        let status = response.status();

        // Revalidated: the archive sent no body, so reuse the one we have and
        // restart its freshness window.
        if status == http::StatusCode::NOT_MODIFIED
            && let Some(mut entry) = cached
        {
            tracing::debug!(%url, "not modified");
            entry.fetched_at = SystemTime::now();
            self.write_cache(&entry);
            let body = Arc::new(entry.body);
            self.memory_put(&url, &body);
            return Ok(body);
        }

        if !status.is_success() {
            // A stale copy beats an error: the archive being briefly down
            // shouldn't make a page we already have unreadable.
            if let Some(entry) = cached {
                tracing::warn!(%url, %status, "serving stale cache");
                return Ok(Arc::new(entry.body));
            }
            return Err(Error::Fetch {
                url,
                status: status.as_u16(),
            });
        }

        let header = |name: http::HeaderName| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let entry = CacheEntry {
            etag: header(http::header::ETAG),
            last_modified: header(http::header::LAST_MODIFIED),
            body: response.text().await?,
            url: url.clone(),
            fetched_at: SystemTime::now(),
        };
        self.write_cache(&entry);
        let body = Arc::new(entry.body);
        self.memory_put(&url, &body);
        Ok(body)
    }

    /// POST a form, for the archive's own `search.php` endpoints.
    ///
    /// Cached like a GET — the key folds in the form body, so two different
    /// searches don't collide and the same search twice costs one request.
    /// Conditional revalidation is skipped: a PHP endpoint sends no validators,
    /// so past the freshness window this is a plain refetch.
    pub async fn post_form(
        &self,
        source: Source,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<Arc<String>> {
        let url = absolute_url(source, path)?;
        let key = format!("{url}#form:{}", serde_json::to_string(form)?);

        if let Some(body) = self.memory_get(&key) {
            tracing::debug!(%url, "search memory hit");
            return Ok(body);
        }

        if let Some(entry) = self.read_cache(&key)
            && entry.is_fresh()
        {
            tracing::debug!(%url, "search disk hit");
            let body = Arc::new(entry.body);
            self.memory_put(&key, &body);
            return Ok(body);
        }

        let _permit = self
            .limiter
            .acquire()
            .await
            .map_err(|e| Error::Cache(e.to_string()))?;

        let response = self.http.post(&url).form(form).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Fetch {
                url,
                status: status.as_u16(),
            });
        }

        let entry = CacheEntry {
            url: key,
            body: response.text().await?,
            etag: None,
            last_modified: None,
            fetched_at: SystemTime::now(),
        };
        self.write_cache(&entry);
        let body = Arc::new(entry.body);
        self.memory_put(&entry.url, &body);
        Ok(body)
    }

    fn read_cache(&self, key: &str) -> Option<CacheEntry> {
        let path = self.cache.as_ref()?.join(cache_key(key));
        let bytes = std::fs::read(path).ok()?;
        // A corrupt or superseded entry is a cache miss, never an error.
        serde_json::from_slice(&bytes).ok()
    }

    /// Best-effort. A cache that can't be written is a performance problem, not
    /// a correctness one, so failures are logged and swallowed.
    fn write_cache(&self, entry: &CacheEntry) {
        let Some(dir) = &self.cache else { return };
        let path = dir.join(cache_key(&entry.url));
        match serde_json::to_vec(entry) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    tracing::debug!(?path, error = %e, "could not write cache");
                }
            }
            Err(e) => tracing::debug!(error = %e, "could not serialise cache entry"),
        }
    }

    /// Delete every cached page. Returns how many entries went.
    pub fn clear_cache(&self) -> Result<usize> {
        let Some(dir) = &self.cache else { return Ok(0) };
        let mut removed = 0;
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                std::fs::remove_file(&path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Resolve `path` against `source` into an absolute URL.
///
/// Accepts a site-absolute path or a full URL already belonging to a known
/// source. A full URL pointing anywhere else is refused rather than fetched.
pub fn absolute_url(source: Source, path: &str) -> Result<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return match Source::of_url(path) {
            Some(_) => Ok(path.to_string()),
            None => Err(Error::ForeignPath(path.to_string())),
        };
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    Ok(format!("{}{path}", source.base_url()))
}

/// Resolve a link `href` found on a page at `base` into a site-absolute path.
///
/// The archive is written with relative links — `../../lloyd/songs/x.html` from
/// a page three directories deep — so every href has to be resolved against
/// where it was found before it can be used as an id. Fragments are kept: one
/// album page routinely documents several releases and the fragment is what
/// tells them apart.
pub fn resolve_path(href: &str, base: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with('/') {
        return href.to_string();
    }

    // Directory of the base page: drop the filename unless base names a
    // directory already.
    let mut segments: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    if !base.ends_with('/') {
        segments.pop();
    }

    // Split the fragment off before walking: `..` inside `#foo` isn't a path
    // step, and a fragment must survive to the end.
    let (path, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (href, None),
    };

    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }

    let resolved = format!("/{}", segments.join("/"));
    match fragment {
        Some(f) => format!("{resolved}#{f}"),
        None => resolved,
    }
}

/// Run `futures` concurrently, returning results in the order given.
///
/// Order matters: it is what makes a page of results reproducible across runs
/// rather than dependent on which request happened to settle first.
pub async fn fan_out<T>(futures: Vec<impl Future<Output = T>>) -> Vec<T> {
    futures_util::future::join_all(futures).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_url_accepts_paths_with_or_without_a_leading_slash() {
        let mn = Source::MainlyNorfolk;
        assert_eq!(
            absolute_url(mn, "/folk/songs/").unwrap(),
            "https://www.mainlynorfolk.info/folk/songs/"
        );
        assert_eq!(
            absolute_url(mn, "folk/songs/").unwrap(),
            "https://www.mainlynorfolk.info/folk/songs/"
        );
    }

    #[test]
    fn absolute_url_passes_through_known_hosts_and_refuses_others() {
        let mn = Source::MainlyNorfolk;
        let ours = "https://www.waterwaysongs.info/Songs/A/alice_white.htm";
        assert_eq!(absolute_url(mn, ours).unwrap(), ours);
        // The open-proxy guard: a model-composed path must not reach the wider web.
        assert!(matches!(
            absolute_url(mn, "https://evil.example.com/"),
            Err(Error::ForeignPath(_))
        ));
    }

    #[test]
    fn relative_links_resolve_against_the_page_that_held_them() {
        let base = "/martin.carthy/songs/theelfinknight.html";
        assert_eq!(
            resolve_path("../../lloyd/songs/themountainshigh.html", base),
            "/lloyd/songs/themountainshigh.html"
        );
        assert_eq!(
            resolve_path("gilbrenton.html", "/folk/songs/childindex.html"),
            "/folk/songs/gilbrenton.html"
        );
        // A directory base keeps its last segment.
        assert_eq!(
            resolve_path("childindex.html", "/folk/songs/"),
            "/folk/songs/childindex.html"
        );
    }

    #[test]
    fn fragments_survive_resolution() {
        // One album page documents several releases; the fragment is the id.
        assert_eq!(
            resolve_path(
                "../../folk/records/redrice.html#rice",
                "/eliza.carthy/songs/x.html"
            ),
            "/folk/records/redrice.html#rice"
        );
        // `..` in a fragment is not a path step.
        assert_eq!(
            resolve_path("x.html#a..b", "/folk/songs/"),
            "/folk/songs/x.html#a..b"
        );
    }

    #[test]
    fn absolute_and_scheme_relative_hrefs_are_left_alone() {
        assert_eq!(
            resolve_path("/folk/", "/martin.carthy/songs/x.html"),
            "/folk/"
        );
        let ext = "https://mudcat.org/thread.cfm?threadid=1";
        assert_eq!(resolve_path(ext, "/folk/songs/"), ext);
    }

    #[test]
    fn the_memory_cache_evicts_oldest_first_and_stays_bounded() {
        let mut cache = MemoryCache::default();
        for i in 0..MEMORY_CACHE_ENTRIES + 10 {
            cache.insert(format!("/page/{i}"), Arc::new(String::new()));
        }
        assert_eq!(cache.entries.len(), MEMORY_CACHE_ENTRIES);
        assert!(cache.get("/page/0").is_none(), "oldest should have gone");
        assert!(cache.get("/page/511").is_some(), "newest should be held");
    }

    #[test]
    fn re_inserting_a_page_does_not_grow_the_eviction_queue() {
        // Otherwise a hub page fetched repeatedly would evict the whole cache
        // by filling `order` with its own duplicates.
        let mut cache = MemoryCache::default();
        for _ in 0..50 {
            cache.insert("/folk/songs/".into(), Arc::new(String::new()));
        }
        assert_eq!(cache.order.len(), 1);
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn a_fresh_entry_is_served_without_asking_the_archive() {
        let fresh = CacheEntry {
            url: "u".into(),
            body: "b".into(),
            etag: None,
            last_modified: None,
            fetched_at: SystemTime::now(),
        };
        assert!(fresh.is_fresh());

        let stale = CacheEntry {
            fetched_at: SystemTime::now() - FRESH_FOR - Duration::from_secs(1),
            ..fresh
        };
        assert!(!stale.is_fresh());
    }
}
