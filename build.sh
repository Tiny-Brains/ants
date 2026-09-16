#!/bin/sh
# The gate, then every artifact, into dist/.
#
# Needs the wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`), `wasm-tools`
# (`cargo install wasm-tools`) and Python 3.11 or newer. The viewer is built separately, by
# viz/build.sh, into dist/viz/.
#
# dist/ IS THE ARTIFACT SET, LAID OUT AS THE IMAGE CARRIES IT: `Dockerfile` runs this script and
# copies dist/ to /artifacts/ unchanged. So a games registry can point `path` at this dist/ or at an
# image's extracted /artifacts/ and read the same tree. What this writes is still not what SHIPS --
# only the image pins the toolchain and remaps build paths, so only the image's digest is the
# platform's -- but it is the same shape, and it is what you run while working here.
#
# The host tests are the gate: nothing at run time will catch a rule implemented wrongly. The
# determinism check runs first, because it is a property of the source rather than of a test.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dist="$here/dist"
target="${CARGO_TARGET_DIR:-$here/engine/target}"

"$here/tools/deny.sh"
cd "$here/engine"
cargo test

# From scratch: a board deleted from maps/ must not survive in dist/maps/, and a viewer transpiled
# from the previous component is a viewer for some other engine.
rm -rf "$dist"
mkdir -p "$dist/reference"

cargo build --release --target wasm32-unknown-unknown --lib
wasm-tools component new "$target/wasm32-unknown-unknown/release/tb_ants.wasm" -o "$dist/tb-ants.wasm"
wasm-tools validate "$dist/tb-ants.wasm" --features component-model

# The manifests are generated, because an artifact nobody hand-edits cannot drift from the code
# that produced it. The reference observations admission validates an adapter against are engine
# output for the same reason: a hand-kept copy would test a shape the game no longer produces.
cargo run --quiet --bin manifest > "$dist/cartridge.json"
cargo run --quiet --bin reference > "$dist/reference/observations.json"

python3 "$here/tools/package.py"
