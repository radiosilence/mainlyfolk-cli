# mainlyfolk-cli

A CLI and MCP server over [Mainly Norfolk](https://www.mainlynorfolk.info/folk/),
Reinhard Zierke's encyclopaedic archive of English folk music, plus
[Waterways Songs](https://www.waterwaysongs.info) for canal and inland-waterways
songs. Both sites are hand-maintained HTML with no API; this gives them a
scriptable JSON-output CLI and a [Model Context Protocol](https://modelcontextprotocol.io)
server exposing one composable GraphQL interface instead of a tool per
operation.

## Install

```bash
cargo install mainlyfolk-cli
```

Or grab a binary from [releases](https://github.com/radiosilence/mainlyfolk-cli/releases),
or pull the image:

```bash
docker pull ghcr.io/radiosilence/mainlyfolk-cli
```

## CLI usage

The binary is `folk`. All output is JSON.

```bash
folk search "reynardine"                    # title search
folk search --scheme roud --number 12       # Roud 12 — The Elfin Knight / Scarborough Fair
folk song /martin.carthy/songs/reynardine.html
folk song /martin.carthy/songs/reynardine.html --lyrics

folk child 84                               # Child 84 — Barbara Allen
folk child 1-50                             # a range
folk laws P15                               # Laws P15 — Reynardine / The Mountains High

folk artist "Martin Carthy"                 # index page + chronological discography
folk records "carthy"                       # search releases by artist or album
folk album /martin.carthy/records/martincarthy.html

folk labels                                 # every label the archive has a discography for
folk books                                  # the bibliography — books the archive's song pages cite
folk books "Ballads and Songs"              # filter by section, author or title
folk waterways "hard working boater"        # canal songs from waterwaysongs.info

folk page /folk/latestchanges.html          # any archive page as plain text
folk latest                                 # what the archive changed recently

folk cache                                  # report cache stats
folk cache --clear                          # empty the on-disk page cache
folk completions zsh
```

Every command that reads the archive also accepts `--no-cache`, global, to
bypass the disk cache for that run.

## MCP server

```bash
folk mcp                             # stdio, for Claude Desktop / Claude Code
folk mcp --http                      # streamable HTTP MCP at /mcp
folk mcp --http 0.0.0.0:8080         # explicit address
folk mcp --graphiql --browser        # GraphiQL IDE, opened for you, no MCP listener needed
```

`--http`, `--graphql` and `--graphiql` are independent surfaces that happen to
share a port: `--http` puts the MCP streamable-HTTP transport on `/mcp`,
`--graphql` serves plain GraphQL-over-HTTP at `/graphql` for anything that
isn't speaking MCP, and `--graphiql` adds the browsable IDE at `/`. Asking for
any of them binds a listener — there's nowhere to mount an HTTP route over
stdio.

The schema is introspectable, so point `--graphiql` at it rather than reading
a hand-written field list here; one goes stale, the other doesn't.

Claude Desktop, in `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "folk": {
      "command": "folk",
      "args": ["mcp"]
    }
  }
}
```

Or with Claude Code:

```bash
claude mcp add --scope user folk -- folk mcp
```

## Caching and politeness

mainlynorfolk.info is one person's decades-long labour, hand-maintained, with
no API and no published rate limit, and its content changes a few times a
month — so nearly every read past the first should never leave the process.
Four layers enforce that, cheapest first:

- **An in-memory cache**, bounded at 512 pages. This is what makes a deep
  GraphQL query that revisits the same hub pages — the Child index, an
  artist's discography, the album every track on it points back to — free
  after the first visit, rather than a disk read per field.
- **A disk cache**, under the platform cache directory (`~/.cache/mainlyfolk`
  on Linux, `~/Library/Caches/mainlyfolk` on macOS), keyed by URL and
  surviving process exit — a session that looks at the Child index ten times
  over a week fetches it once, and a long-running MCP server keeps that
  benefit across restarts.
- **Conditional revalidation**, once the disk entry passes 24 hours old: the
  request carries `If-None-Match`/`If-Modified-Since`, and the archive answers
  `304` with no body — the cheapest thing it can be asked to do.
- **A real fetch**, capped at 4 concurrent requests across everything. A
  single GraphQL query can fan out into dozens of page loads (an artist's
  whole discography, a song's every recording); this is what stops that from
  hitting a small static host all at once.

Every request also carries a `User-Agent` naming the tool and this repo, so
an archive maintainer who notices it in their logs can see what it is.

`--no-cache` disables the disk cache only. The in-memory cache holds nothing
across runs, so it has nothing stale to serve — there is no reason to disable
it too.

## Credit

This archive exists because Reinhard Zierke has spent decades cataloguing
English folk song, and [Mainly Norfolk](https://www.mainlynorfolk.info/folk/)
is the result. This tool is a reader over that work, nothing more.

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

`tests/live_archive.rs` checks the running assumptions against the real
archives — that `search.php` still answers, that an entry-point page is still
there. It's `#[ignore]`d and excluded from CI on purpose: these hit a
volunteer-run site, and scheduled traffic against it would be the opposite of
the politeness this tool otherwise goes out of its way for. Run it by hand
when something looks wrong:

```bash
cargo test --test live_archive -- --ignored --test-threads=1
```

## License

MIT
