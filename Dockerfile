# mainlynorfolk-mcp as a container. No build stage, no package manager — CI
# builds the static musl binary and this image only copies it in. `docker
# build .` by hand requires dist/ to be populated first; that is intended,
# docker builds only ever happen in CI.

FROM scratch

ARG TARGETARCH

# VALIDATED, DO NOT "SIMPLIFY" THIS AWAY:
# rustls-platform-verifier requires a system trust store on Linux. It does NOT
# fall back to the webpki roots compiled into the binary. With no CA bundle on
# disk, reqwest panics before making a single request:
#   Client::new(): reqwest::Error { kind: Builder,
#     source: General("No CA certificates were loaded from the system") }
# Verified by running the static binary on bare scratch against a real HTTPS
# host: without this line it panics; with it, TLS completes and the server's
# own auth response comes back. Sourced from distroless/static so we need no
# package manager and no build stage — it is a plain copy from a published,
# CVE-maintained image.
COPY --from=gcr.io/distroless/static:latest \
     /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

# The archive client keeps an on-disk page cache under dirs::cache_dir() (see
# src/client.rs). scratch has no shell to mkdir one and no HOME to resolve it
# against, so without this the cache silently no-ops and every request
# hammers mainlynorfolk.info and waterwaysongs.info — hand-maintained
# volunteer sites with no published rate limit. /home/nonroot in
# distroless/static:nonroot is a real, writable (mode 0700) directory; the
# --chown re-owns it to the uid this image runs as.
COPY --from=gcr.io/distroless/static:nonroot --chown=10001:10001 \
     /home/nonroot /home/app/.cache
ENV HOME=/home/app
ENV XDG_CACHE_HOME=/home/app/.cache
VOLUME ["/home/app/.cache"]

COPY dist/folk-linux-${TARGETARCH}-musl /folk

EXPOSE 8080

# scratch has no /etc/passwd, so this must be a raw numeric uid.
USER 10001:10001

LABEL org.opencontainers.image.title="mainlynorfolk-mcp"
LABEL org.opencontainers.image.vendor="James Cleveland"
LABEL org.opencontainers.image.licenses="MIT"
LABEL org.opencontainers.image.source="https://github.com/radiosilence/mainlynorfolk-mcp"

ENTRYPOINT ["/folk"]
CMD ["--http", "0.0.0.0:8080"]
