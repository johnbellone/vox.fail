# One image for all of Vox's servers: vox-directory (accounts, the pages, the game's files) and
# vox-server (one world each), with the assets, the game data, the levels and the built site
# (including the browser client). The Helm chart (deploy/helm/vox) runs both from it, so the
# directory and every world are always the same release.
#
#   docker build -f deploy/docker/Dockerfile -t ghcr.io/<you>/vox:<version> .
#
# Build from the repository's root. The browser client is the slow part (Bevy for wasm32,
# about ten minutes the first time); `--build-arg WEB=skip` leaves it out (the pages still
# work, the play page and the front page's live view don't).

ARG RUST_VERSION=1.100
ARG BUN_VERSION=1.4.5
ARG DEBIAN=bookworm
ARG WASM_BINDGEN=0.2.130

# ---- the servers
FROM rust:${RUST_VERSION}-${DEBIAN} AS servers
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p vox-server -p vox-directory \
    && mkdir -p /out && cp target/release/vox-server target/release/vox-directory /out/

# ---- the browser client (WebAssembly), into apps/site/public/pkg
FROM rust:${RUST_VERSION}-${DEBIAN} AS client
ARG WASM_BINDGEN
ARG WEB=build
WORKDIR /src
RUN if [ "$WEB" = build ]; then \
      rustup target add wasm32-unknown-unknown \
      && cargo install --locked wasm-bindgen-cli --version "$WASM_BINDGEN" \
      && apt-get update && apt-get install -y --no-install-recommends brotli && rm -rf /var/lib/apt/lists/*; \
    fi
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    mkdir -p apps/site/public/pkg \
    && if [ "$WEB" = build ]; then tools/dev/build_web.sh; fi \
    && mkdir -p /out && cp -r apps/site/public/pkg /out/pkg

# ---- the pages
FROM oven/bun:${BUN_VERSION} AS site
WORKDIR /src
COPY package.json bun.lock ./
COPY apps/site/package.json apps/site/
RUN bun install --frozen-lockfile
COPY apps/site apps/site
# the pages use the app's icon
COPY assets/icon.png assets/icon.png
COPY --from=client /out/pkg apps/site/public/pkg
RUN cd apps/site && bun run build

# ---- what runs
FROM debian:${DEBIAN}-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /var/lib/vox --create-home vox
WORKDIR /opt/vox
COPY --from=servers /out/ /opt/vox/bin/
COPY assets /opt/vox/assets
COPY data /opt/vox/data
COPY --from=site /src/apps/site/dist /opt/vox/web
ENV PATH=/opt/vox/bin:$PATH
USER 10001
VOLUME /var/lib/vox
# tini passes SIGTERM on, and both servers save and stop on it
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["vox-directory", "--listen", "0.0.0.0:7700", "--db", "/var/lib/vox/directory.db", "--key", "/var/lib/vox/directory.key", "--web", "/opt/vox/web", "--assets", "/opt/vox/assets", "--data", "/opt/vox/data"]
