# mainlynorfolk-mcp as a container. The default command runs the MCP server over
# HTTP. Plain Rust build like caldav-cli's — no native toolchain needed.
#
# The archive client caches fetched pages on disk under the user's cache dir
# (see src/client.rs). In a container that means XDG_CACHE_HOME needs to
# point somewhere the non-root user can write; without that the cache
# silently no-ops on every write and every page is refetched, which is
# correct-but-slow behaviour, not a failure.

FROM rust:1-bookworm AS build
WORKDIR /app
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/folk /usr/local/bin/folk
EXPOSE 8080
RUN useradd --system --uid 10001 --create-home app \
    && mkdir -p /home/app/.cache && chown app:app /home/app/.cache
ENV XDG_CACHE_HOME=/home/app/.cache
USER app
ENTRYPOINT ["folk"]
CMD ["--http", "0.0.0.0:8080"]
