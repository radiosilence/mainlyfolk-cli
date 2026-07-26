# mainlynorfolk-mcp

An MCP server over [Mainly Norfolk](https://www.mainlynorfolk.info/folk/),
Reinhard Zierke's encyclopaedic archive of English folk music, plus
[Waterways Songs](https://www.waterwaysongs.info) for canal and inland-waterways
songs. Neither site has an API; this gives them one composable GraphQL schema
and serves it to a model over MCP.

Two tools, not one per operation: `folk_schema` returns the SDL, `folk` runs a
query. The schema is a few thousand tokens, so it stays behind a tool call — a
session that never mentions folk music pays almost nothing for having this
connected.

## Install

```bash
cargo install mainlynorfolk-mcp   # installs the `folk` binary
```

Or pull the image, or take a binary from
[releases](https://github.com/radiosilence/mainlynorfolk-mcp/releases):

```bash
docker pull ghcr.io/radiosilence/mainlynorfolk-mcp
```

## Connect it

Claude Desktop, in `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "folk": {
      "command": "folk"
    }
  }
}
```

Claude Code:

```bash
claude mcp add --scope user folk -- folk
```

With no arguments it speaks MCP over stdio, which is what both of those launch.

## The graph

The schema is introspectable — point `--graphiql` at it rather than reading a
field list here, because one of those goes stale:

```bash
folk --graphiql --browser
```

What makes it worth querying rather than scraping is that the edges are real.
A song knows the artists who claim it, the releases it appears on, and the books
that cite it; a release knows its tracks, and a track knows its song. The graph
has cycles by design, and depth is the point — `artist → discography → album →
tracks → song → recordings → album` is seven levels before it repeats.

```graphql
{
  song(path: "/lloyd/songs/sailorcutdowninhisprime.html") {
    title
    refs { roud child masterTitle }
    lyrics { performer text }
    sameRoud(first: 5) {          # other pages sharing this Roud number
      nodes { title }
    }
  }
}
```

`sameRoud` is the edge worth knowing about. Roud numbers are the archive's own
"these are the same song" key, so it reassembles a family: Roud 2 gathers
*Young Sailor Cut Down in His Prime*, *Bright Shiny Morning*, *When I Was on
Horseback* and *Young Girl Cut Down in Her Prime* — the ballad that became
*Streets of Laredo*. It costs one search rather than a crawl, and it recurses
through `SongSummary.song`.

Reads are Relay connections: `first`/`after`, and a `totalCount` that is free
because these lists arrive whole from one request. Cursors are paths, so a stale
one tells you to restart rather than quietly returning a different page.

Searching uses the archive's own `search.php` endpoints in preference to
fetching an index: the full song index is 670KB of HTML, and the same query
server-side is 10KB. Reference searches accept Roud, Child, Laws, Greig-Duncan
and Sam Henry numbers.

## Serving it elsewhere

```bash
folk --http                  # MCP over streamable HTTP at /mcp
folk --http 0.0.0.0:8080     # explicit address
folk --graphql               # plain GraphQL-over-HTTP at /graphql
folk --graphiql --browser    # the IDE, opened for you
```

`--http`, `--graphql` and `--graphiql` are independent surfaces that happen to
share a port. `--http` puts MCP's streamable-HTTP transport on `/mcp`;
`--graphql` serves anything speaking GraphQL directly rather than through MCP's
JSON-RPC envelope; `--graphiql` adds the IDE at `/`. Asking for any of them
binds a listener, because there is nowhere to mount an HTTP route over stdio.

Introspection is answered from the schema without touching the archive, so the
IDE's docs and autocomplete work before a single page is fetched.

## Caching and politeness

mainlynorfolk.info is one person's decades-long labour, hand-maintained, with no
API and no published rate limit, and its content changes a few times a month —
so nearly every read past the first should never leave the process. Four layers
enforce that, cheapest first:

- **An in-memory cache**, bounded at 512 pages. A deep query revisits the same
  hub pages constantly — the Child index, an artist's discography, the album
  every track points back to — and this makes the second visit free.
- **A disk cache** under the platform cache directory (`~/.cache/mainlynorfolk`
  on Linux, `~/Library/Caches/mainlynorfolk` on macOS), surviving process exit,
  so a long-running server keeps the benefit across restarts.
- **Conditional revalidation** once a disk entry passes 24 hours: the request
  carries `If-None-Match`/`If-Modified-Since` and the archive answers `304` with
  no body.
- **A real fetch**, capped at 4 concurrent across everything. A single query can
  fan out into dozens of page loads; this is what stops that arriving all at
  once.

Every request carries a `User-Agent` naming the tool and this repo. It does not
pretend to be a browser.

`folk clear-cache` empties the disk cache; `folk completions <shell>` writes
shell completions.

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

Parsers are pure `&str` → model functions tested against saved fixtures in
`tests/fixtures/`, so the suite needs no network.

`tests/live_archive.rs` checks the running assumptions against the real sites —
that `search.php` still answers, that the entry points are still there. It is
`#[ignore]`d and excluded from CI on purpose: these hit a volunteer-run site,
and scheduled traffic against it would be the opposite of the politeness this
tool otherwise goes out of its way for. Run it by hand when something looks
wrong:

```bash
cargo test --test live_archive -- --ignored --test-threads=1
```

## Credit

This archive exists because Reinhard Zierke has spent decades cataloguing
English folk song, and [Mainly Norfolk](https://www.mainlynorfolk.info/folk/) is
the result. This is a reader over that work, nothing more.

## License

MIT
