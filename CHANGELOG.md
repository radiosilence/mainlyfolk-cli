# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] - 2026-07-26

### Fixed

- **No image was ever published under the new name.** 1.0.0 was released while
  the repository was still `mainlynorfolk-cli`, so its images went to the
  `mainlynorfolk-cli` package and the rename that followed left
  `ghcr.io/radiosilence/mainlynorfolk-mcp:v1.0.0` referring to something that
  does not exist — a GHCR package does not follow its repository. Anything
  pinned to the correct-looking name could not pull it. This release exists to
  publish under the right one; the code is unchanged.

## [1.0.0] - 2026-07-26

Renamed from `mainlynorfolk-cli` to `mainlynorfolk-mcp`, and reduced to the one
thing it is actually used for.

### Changed

- **The binary is the MCP server.** Running it with no arguments speaks MCP over
  stdio; `--http`, `--graphql` and `--graphiql` are flags rather than
  subcommands of an `mcp` subcommand. What was `folk mcp --http` is now
  `mainlynorfolk-mcp --http`.
- **The crate, binary and image are `mainlynorfolk-mcp`.** The old `folk`
  binary is gone. The MCP *tools* are still named `folk` and `folk_schema` —
  those are what a model sees and they did not need renaming.

### Removed

- **The CLI.** Every subcommand — `search`, `song`, `child`, `laws`, `artist`,
  `records`, `album`, `labels`, `books`, `waterways`, `page`, `latest` — and the
  `Output` JSON envelope they printed. Nothing used them: the GraphQL schema
  answers all of it more precisely, and each subcommand was a second, thinner
  way to ask the same question that had to be kept in step with the first.
  `--clear-cache` survives as a flag, being maintenance rather than a query.
- **The pass-through accessors on `Archive`.** Loaders already fetched through
  the page loader and called the parsers directly, so `song()`, `artist()`,
  `album()`, `page()` and the index readers existed only for the CLI. What
  remains is what an API call actually is here: the two `search.php` endpoints,
  which are POSTs rather than paths, and the paths themselves.

There is now one path through the crate — GraphQL resolver, to DataLoader, to
API call — instead of two arriving from different directions.

## [0.1.1] - 2026-07-25

### Fixed

- **All-caps record labels kept their catalogue number.** `HMV DLP 1143` was
  read as a label with no catalogue number at all, because the right-to-left
  scan that finds a catalogue number cannot tell `HMV` from `DLP` — a
  mixed-case label like `Fontana` stops the scan, an all-caps one never does.
  The first word is now kept as the label in that case. Four of Shirley
  Collins' first twelve releases were affected.
- **`Roud -` is no longer stored as the reference number `-`.** The archive
  writes a bare dash where it has no number, and a dash stored as a Roud number
  looks like data while matching nothing — including through `Song.sameRoud`,
  which feeds it back into the archive's own search.
- **An artist's life dates are no longer part of their name.** The archive puts
  them on a second line of the heading (`Ewan MacColl<br>(25 January 1915 - 22
  October 1989)`), and a `<br>` contributes no text, so they were being
  concatenated onto the name and carried into every attribution naming him.

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
  `labels`, `books`, `waterways`, `page`, `latest`. Song and record search go
  through the archive's own `search.php` endpoints rather than downloading and
  filtering the full index client-side. `books` browses the bibliography — the
  works the archive's song pages cite — filterable by section, author or
  title.
- **Two caches in front of every fetch, memory then disk.** A process-wide
  in-memory cache, bounded at 512 pages, makes a deep GraphQL query that
  revisits the same hub pages — the Child index, an artist's discography —
  free after the first visit. Behind it, a disk cache keyed by URL under the
  platform cache directory survives process exit. Past a 24-hour freshness
  window, requests revalidate conditionally (`If-None-Match`/
  `If-Modified-Since`) instead of refetching outright, and a stale copy is
  served rather than an error if the archive is briefly unreachable.
  `--no-cache` disables the disk layer only; the in-memory cache holds nothing
  across runs, so it has nothing stale to serve.
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
