# mainlynorfolk-mcp as a container. The default command runs the MCP server over
# HTTP. This is an HTTP *client* (it scrapes mainlynorfolk.info and
# waterwaysongs.info), so the CA bundle must be present at runtime —
# rustls-platform-verifier reads the system trust store.
#
# The archive client caches fetched pages on disk under the user's cache dir
# (see src/client.rs). scratch has no /etc/passwd and no HOME, so
# dirs::cache_dir() would return None and the cache would silently no-op on
# every write — hammering a volunteer-run site on every request. HOME and
# XDG_CACHE_HOME are set explicitly below to keep that cache alive.

# Selects which stage supplies the binary. Must be declared before the first
# FROM to be usable in one. `docker build .` compiles from source; CI passes
# prebuilt to reuse the binary the release matrix already built.
ARG BIN_SOURCE=source

# Source build.
FROM rust:1-slim AS builder

# musl-tools for the static target; cmake/clang for aws-lc-sys, the rustls
# crypto backend, which is a C/C++ build.
RUN apt-get update && apt-get install -y --no-install-recommends \
    musl-tools musl-dev cmake clang ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add $(uname -m)-unknown-linux-musl

WORKDIR /build
COPY . .

RUN TARGET=$(uname -m)-unknown-linux-musl && \
    cargo build --release --locked --target $TARGET && \
    cp target/$TARGET/release/folk /tmp/folk

# Empty dir to seed the cache directory on scratch, which has no mkdir.
RUN mkdir -p /emptydir

FROM scratch AS bin-source
COPY --from=builder /tmp/folk /folk

FROM scratch AS bin-prebuilt
ARG TARGETARCH
COPY dist/folk-linux-${TARGETARCH}-musl /folk

# Runtime stage. BuildKit only builds the stage this resolves to, so the
# source build is skipped entirely when BIN_SOURCE=prebuilt.
FROM bin-${BIN_SOURCE}

# rustls-platform-verifier reads the system trust store, so the bundle has to
# be in the image. Sourced from the builder rather than pinned separately, so
# it refreshes whenever the base image does.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# Disk cache directory. scratch has no /etc/passwd, so USER must be a raw
# numeric uid, and HOME/XDG_CACHE_HOME must be set explicitly or
# dirs::cache_dir() returns None and the cache silently no-ops.
COPY --from=builder --chown=10001:10001 /emptydir /home/app/.cache
ENV HOME=/home/app
ENV XDG_CACHE_HOME=/home/app/.cache
VOLUME ["/home/app/.cache"]

EXPOSE 8080

LABEL org.opencontainers.image.vendor="James Cleveland"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.source="https://github.com/radiosilence/mainlynorfolk-mcp"

USER 10001:10001
ENTRYPOINT ["/folk"]
CMD ["--http", "0.0.0.0:8080"]
