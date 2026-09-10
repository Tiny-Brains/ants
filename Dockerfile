# syntax=docker/dockerfile:1

# The Ants cartridge as an ARTIFACT IMAGE: the build output this repository used to commit, carried
# in an image instead of in git.
#
# WHY THIS EXISTS. Every consumer of this cartridge -- kalam's plugin, web's replay viewer, docs'
# lesson player, the tinybrains CLI -- used to read it out of a sibling checkout, which made a
# clone of one repository beside the others the only layout the platform builds in. An image
# reference is a coordinate that works the same locally and remotely: `tinybrains/ants:dev` built
# from this directory, or `ghcr.io/tiny-brains/ants:v1.2.3` pulled, and nothing downstream changes
# but the tag.
#
# THE DIGEST IS STILL DERIVED, NEVER TYPED. Nothing here writes the engine digest into a file for
# someone to copy: the component is the record, `viz/dist/engine.json` names the digest the viewer
# was transpiled from, and every consumer hashes the bytes it actually received. What changes is
# that the bytes now come from ONE BUILD rather than one commit -- so the platform's rule is that
# every consumer takes the same image, not that each one runs this Dockerfile.
#
# THE TOOLCHAIN IS PINNED FOR A REASON. `ants/docs/cartridge.md` §4 makes the component's bytes
# load-bearing: a replica whose engine digest is not `games.active_engine_digest` claims nothing,
# for ever, and the queue grows. A rustc bump moves those bytes even when no rule changed, so it is
# a deliberate edit here rather than whatever the builder happened to have.

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
# deny.sh's determinism law, then the host tests -- and a Dockerfile that skipped to the cargo line
# would be a second, weaker build of the same artifact.
FROM rust:${RUST_VERSION}-trixie AS engine
ARG WASM_TOOLS_VERSION
ARG TARGETARCH

# python3 for the generators (embed-maps, plugin-json, cartridge, report); jsonschema so
# schema/validate.py actually runs instead of printing "skipped".
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-jsonschema \
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
# therefore two different engine digests. That is not hypothetical: the artifact this repository
# committed carries `/Users/<someone>/.cargo/...`, which nobody else can reproduce.
#
# Remapping them to fixed names makes the component a function of the SOURCE, which is what every
# consumer already assumes it is.
ENV RUSTFLAGS="--remap-path-prefix=/usr/local/cargo/registry/src=/cargo --remap-path-prefix=/usr/local/rustup/toolchains=/rustup --remap-path-prefix=/src=/ants"

WORKDIR /src
COPY . .

# reference/ reaches the context as an empty directory (its only file is a build artifact, and
# .dockerignore drops it), and a build context does not reliably carry empty directories. build.sh
# REDIRECTS into it, so it has to exist before the redirect is opened.
RUN mkdir -p reference

# The registry and target caches make an edit-and-rebuild cost a recompile rather than a cold
# build. Neither is part of the image: build.sh writes every artifact beside the source, not under
# target/, which is why the carrier stage below can copy them out.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    ./build.sh

# ---- the replay viewer -------------------------------------------------------
#
# A separate stage because it needs Node and the engine stage does not, and because the viewer is
# transpiled FROM the component: viz/build.sh takes the wasm as its argument and writes the digest
# it saw into dist/engine.json, so a viewer built against some other engine is caught downstream by
# a digest comparison rather than by drawing a plausible match that never happened.
FROM node:${NODE_VERSION}-alpine AS viz
# The repository's own layout, not a flattened viz/: check.mjs replays
# ../tests/fixtures/replay-maze-03.json through the geometry it is checking, so the fixture has to
# sit where the checkout puts it.
WORKDIR /src/viz
COPY viz/ ./
COPY tests/fixtures/ /src/tests/fixtures/
COPY --from=engine /src/tb-ants.wasm /tb-ants.wasm
# jco's version is pinned inside viz/build.sh; it stays the single place that names it.
RUN ./build.sh /tb-ants.wasm

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

COPY --from=engine /src/tb-ants.wasm /src/plugin.json /src/cartridge.json /artifacts/
COPY --from=engine /src/maps/      /artifacts/maps/
COPY --from=engine /src/reference/ /artifacts/reference/
COPY --from=viz    /src/viz/dist/  /artifacts/viz/

# `docker run --rm -v ants-engine:/out tinybrains/ants:dev` populates a volume with everything.
# Consumers that want a subset pass their own command; kalam takes the three plugin files.
CMD ["sh", "-c", "cp -a /artifacts/. /out/"]
