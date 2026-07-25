# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-07-25

The initial feature set rather than a set of changes against a released
version.

### Added

- **One binary, two front ends.** A JSON-output CLI (`folk`) and an MCP server
  over the same archive client, exposing one composable GraphQL schema instead
  of a tool per operation.
- **Two archives.** [Mainly Norfolk](https://www.mainlynorfolk.info/folk/),
  Reinhard Zierke's English folk song archive, and
  [Waterways Songs](https://www.waterwaysongs.info) for canal and
  inland-waterways songs.
- **Reads**: `search`, `song`, `child`, `laws`, `artist`, `records`, `album`,
  `labels`, `waterways`, `page`, `latest`. Song and record search go through
  the archive's own `search.php` endpoints rather than downloading and
  filtering the full index client-side.
- **A disk cache in front of every fetch.** Pages are keyed by URL under the
  platform cache directory and survive process exit. Past a 24-hour freshness
  window, requests revalidate conditionally (`If-None-Match`/
  `If-Modified-Since`) instead of refetching outright, and a stale copy is
  served rather than an error if the archive is briefly unreachable.
- **A concurrency cap of 4** across every request, so a single GraphQL query
  fanning out into many page loads can't turn into a burst against a
  hand-maintained static site.
- **`cache`**, to inspect the politeness settings in effect or clear the
  on-disk cache.
- **Shell completions** for bash, zsh, fish, and others clap supports.
- **HTTP surfaces for the MCP server**: `--http` for streamable-HTTP MCP,
  `--graphql` for plain GraphQL-over-HTTP, `--graphiql` for a browsable IDE,
  `--browser` to open it automatically. Each is independent and they share a
  port, since a model's transport and a human's browser tab are different
  things.
- Supersedes an earlier MCP-only TypeScript implementation of the same idea.
