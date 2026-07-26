# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.2] (2026-07-26)

### Changed

- **Fat LTO with serialized codegen is the slowest possible build configuration.**
  `lto = true` and `codegen-units = 1` trigger whole-program optimization across
  the entire dependency tree in a single pass — CI builds measured 329s of pure
  `cargo build` per target. For an I/O-bound service that gains nothing measurable
  from aggressive optimization, thin LTO and parallel codegen units deliver the
  same binary behavior with dramatically faster builds. Switched to `lto = "thin"`
  and `codegen-units = 16`.

## [1.1.1] (2026-07-26)

### Changed

- **The image is a plain `COPY` onto `scratch` — no build stage, no package
  manager, at all.** CI compiles the static musl binary once in the workflow
  and this Dockerfile only copies it in; a bare `docker build .` requires
  `dist/` to already hold the binary, which is intentional — docker builds
  only ever happen in CI now, so nothing recompiles a binary CI already
  built. Same binary, no shell, nothing to patch, and it drops from a
  debian-slim base to roughly 20MB.
- **The CA bundle and cache directory both come from `gcr.io/distroless/static`
  rather than from any local build step**, so there's nothing to build to get
  them. `rustls-platform-verifier` needs a real system trust store on Linux —
  it does **not** fall back to the webpki roots compiled into the binary —
  confirmed by running the static binary on bare `scratch` with no cert file:
  it panics at `Client::new()` with "No CA certificates were loaded from the
  system" before making a single request.
- **The disk page cache survives the move to `scratch`.** `scratch` has no
  shell to `mkdir` a cache directory and no `/etc/passwd` to resolve `HOME`
  against, which would make `dirs::cache_dir()` return `None` and silently
  stop caching altogether. `/home/nonroot` from `distroless/static:nonroot` is
  a real, writable directory, re-owned to this image's uid and mounted as
  `XDG_CACHE_HOME`, so the cache keeps working exactly as it does today — this
  matters because mainlynorfolk.info and waterwaysongs.info are
  hand-maintained volunteer sites with no published rate limit, and losing the
  cache silently means hammering them on every request.
- **Builds run on every PR; only `main` pushes publish.** A broken Dockerfile
  now fails before merge instead of after — the image build, lint, format and
  test jobs all run per-PR, and only pushing to the registry is gated on
  `main`.

## [1.1.0] - 2026-07-26

### Changed

- **The binary is `folk` again.** The package is still `mainlynorfolk-mcp` —
  that names the archive, and is what you install — but `folk` is what gets
  typed and what a desktop client puts in its config.
- **`completions` and `clear-cache` are subcommands, not flags.**
  `folk completions zsh` is what a hand reaches for, and what the sibling tools
  already accept; `--completions zsh` was neither. The server flags stay flags,
  because they configure the thing that runs rather than replacing it.

### Fixed

- **A stale `Cargo.lock` no longer reaches the image build.** `check` builds and
  tests with `--locked`, so a lockfile that has drifted from `Cargo.toml` — which
  a version bump does every time — fails in the pull request rather than in the
  Docker build, which is the only step that used `--locked` and so the only one
  that noticed.
- **A version tag is never cut without an image behind it.** `publish` now waits
  for the image builds as well as the tarballs. Previously they ran in parallel,
  so v1.0.2 got a GitHub release and no container image at all — and an
  auto-updater polling releases will happily pin one of those.

## [1.0.2] - 2026-07-26

### Fixed

- **`--browser` implies `--graphiql`, which implies `--graphql`.** It previously
  *required* `--graphiql` and errored without it, which meant spelling out three
  flags to mean one thing. Asking for a surface now asks for what it needs:
  opening a browser at the IDE means serving the IDE, which means serving the
  `/graphql` it talks to. Matches how `caldav-cli` and `fastmail-cli` already
  behave.

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
