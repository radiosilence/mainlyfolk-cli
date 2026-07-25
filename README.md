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
folk search --scheme roud --number 12       # Roud 12 — The Twa Corbies / other titles
folk song /martin.carthy/songs/reynardine.html
folk song /martin.carthy/songs/reynardine.html --lyrics

folk child 84                               # Child #84 — Bonny Barbara Allan
folk child 1-50                             # a range
folk laws P15

folk artist "Martin Carthy"                 # index page + chronological discography
folk records "carthy"                       # search releases by artist or album
folk album /martin.carthy/records/carthy.html

folk labels                                 # every label the archive has a discography for
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
no API and no published rate limit — so restraint is this tool's job, not
the archive's. Three things enforce it:

- **A disk cache.** Every fetched page is written under the platform cache
  directory (`~/.cache/mainlyfolk` on Linux, `~/Library/Caches/mainlyfolk` on
  macOS), keyed by URL, and survives process exit — a session that looks at
  the Child index ten times over a week fetches it once.
- **24-hour freshness, then conditional revalidation.** Past that window a
  request carries `If-None-Match`/`If-Modified-Since`; the archive answers
  `304` with no body, the cheapest thing it can be asked to do.
- **A concurrency cap of 4.** A single GraphQL query can fan out into dozens
  of page loads (an artist's whole discography, a song's every recording);
  this is what stops that from hitting a small static host all at once.

Every request also carries a `User-Agent` naming the tool and this repo, so
an archive maintainer who notices it in their logs can see what it is.

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

## License

MIT
