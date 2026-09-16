# syntax=docker/dockerfile:1

# The Ants cartridge as an ARTIFACT IMAGE: the component, its manifests, the boards, the reference
# observations and the replay viewer, under /artifacts/. Consumers name an image rather than a
# checkout -- `tinybrains/ants:dev` built here, or `ghcr.io/tiny-brains/ants:<tag>` pulled -- and
# /artifacts/ is laid out exactly as `./build.sh` lays out dist/, so either one reads the same.
#
# THE DIGEST IS DERIVED, NEVER TYPED. Nothing writes the engine digest into a file for someone to
# copy: the component is the record, `viz/engine.json` names the digest the viewer was transpiled
# from, and every consumer hashes the bytes it received. The bytes come from ONE BUILD, so the
# platform's rule is that every consumer takes the same image, not that each one runs this file.
#
# THE TOOLCHAIN IS PINNED because the component's bytes are load-bearing: a replica whose engine
# digest is not `games.active_engine_digest` claims nothing, for ever. A rustc bump moves those
# bytes even when no rule changed, so it is a deliberate edit here rather than whatever the builder
# happened to have.

# EXACT, not 1.98: `rust:1.98-trixie` floats to the newest patch, and a patch bump moves the
# component's bytes. Bumping this is an engine-digest change and travels on the same rails as a
# rules change -- ENGINE_RELEASE=1, refused while a season is live.
ARG RUST_VERSION=1.98.1
ARG NODE_VERSION=22
ARG WASM_TOOLS_VERSION=1.258.0
ARG BUSYBOX_VERSION=1.37-musl

# ---- the component, the manifests, the reference observations ----------------
#
# `build.sh` is run whole rather than reimplemented in RUN steps. It is this repository's gate --
# the determinism check, then the host tests -- and a Dockerfile that skipped to the cargo line
# would be a second, weaker build of the same artifact.
FROM rust:${RUST_VERSION}-trixie AS engine
ARG WASM_TOOLS_VERSION
ARG TARGETARCH

# python3 for tools/package.py.
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 \
 && rm -rf /var/lib/apt/lists/*

# The prebuilt release, not `cargo install`: the same binary the version pin names, in seconds
# rather than minutes of compiling a tool that is not part of this artifact.
RUN set -eux; \
    case "${TARGETARCH}" in \
      amd64) arch=x86_64 ;; \
      arm64) arch=aarch64 ;; \
      *) echo "wasm-tools publishes no ${TARGETARCH} linux build" >&2; exit 1 ;; \
    esac; \
    name="wasm-tools-${WASM_TOOLS_VERSION}-${arch}-linux"; \
    curl -fsSL "https://github.com/bytecodealliance/wasm-tools/releases/download/v${WASM_TOOLS_VERSION}/${name}.tar.gz" \
      | tar -xz -C /tmp; \
    install -m 0755 "/tmp/${name}/wasm-tools" /usr/local/bin/wasm-tools; \
    rm -rf "/tmp/${name}"; \
    wasm-tools --version

RUN rustup target add wasm32-unknown-unknown

# THE DIGEST MUST NOT DEPEND ON WHERE THE BUILD RAN. rustc bakes the absolute path of every source
# file a panic can name into the binary -- the crates.io checkout and the toolchain's own library
# source included -- so the same commit built in two places produces two different components, and
# therefore two different engine digests. Remapping them to fixed names makes the component a
# function of the SOURCE, which is what every consumer already assumes it is.
ENV RUSTFLAGS="--remap-path-prefix=/usr/local/cargo/registry/src=/cargo --remap-path-prefix=/usr/local/rustup/toolchains=/rustup --remap-path-prefix=/src=/ants"

WORKDIR /src
COPY . .

# The registry and target caches make an edit-and-rebuild cost a recompile rather than a cold
# build. Neither is part of the image: build.sh writes every artifact to dist/, not under
# engine/target/, which is why the carrier stage below can copy them out.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/engine/target,sharing=locked \
    ./build.sh

# ---- the replay viewer -------------------------------------------------------
#
# A separate stage because it needs Node and the engine stage does not, and because the viewer is
# transpiled FROM the component: viz/build.sh writes the digest it saw into viz/engine.json, so a
# viewer built against some other engine is caught downstream by a digest comparison rather than
# by drawing a plausible match that never happened.
FROM node:${NODE_VERSION}-alpine AS viz
# The repository's own layout, not a flattened viz/: check.mjs replays the fixture under
# engine/src/tests/fixtures/ through the geometry it is checking, and build.sh writes ../dist/viz/.
WORKDIR /src/viz
COPY viz/ ./
COPY engine/src/tests/fixtures/ /src/engine/src/tests/fixtures/
COPY --from=engine /src/dist/tb-ants.wasm /src/dist/tb-ants.wasm
# jco's version is pinned inside viz/build.sh; it stays the single place that names it.
RUN ./build.sh

# ---- the carrier -------------------------------------------------------------
#
# busybox rather than scratch: this image is consumed two ways, and one of them needs a shell. A
# Dockerfile takes it as a named build context (`COPY --from=ants /artifacts/...`), which scratch
# would serve; compose runs it as a one-shot that copies itself into a shared volume, for the
# consumers that are bind-mounted into an Orion container rather than built.
FROM busybox:${BUSYBOX_VERSION}
LABEL org.opencontainers.image.title="tb.ants cartridge artifacts" \
      org.opencontainers.image.source="https://github.com/Tiny-Brains/ants" \
      org.opencontainers.image.description="tb-ants.wasm, its manifests, the board catalogue, the reference observations, and the replay viewer"

COPY --from=engine /src/dist/     /artifacts/
COPY --from=viz    /src/dist/viz/ /artifacts/viz/

# `docker run --rm -v ants-engine:/out tinybrains/ants:dev` populates a volume with everything.
# Consumers that want a subset pass their own command; kalam takes the three plugin files.
CMD ["sh", "-c", "cp -a /artifacts/. /out/"]
