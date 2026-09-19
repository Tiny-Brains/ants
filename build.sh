#!/bin/sh
# The gate, then every artifact, into dist/.
#
# Needs rustup (rust-toolchain.toml names the exact compiler and the wasm32 target), `wasm-tools`
# at WASM_TOOLS_VERSION below, and Python 3.11 or newer. The viewer is built separately, by
# viz/build.sh, into dist/viz/.
#
# dist/ IS THE ARTIFACT SET, AND THIS SCRIPT IS THE BUILD THAT SHIPS. A release is this script and
# viz/build.sh run by .github/workflows/build.yml, with dist/ packed into one archive by
# tools/pack.py. There is no second build of the same artifact anywhere.
#
# THE ENGINE DIGEST IS A FUNCTION OF FOUR THINGS: the source, the exact rustc, the exact wasm-tools,
# and the HOST rustc runs on. The first three are pinned here and in rust-toolchain.toml, and the
# build paths are remapped below. The host cannot be: the same source, flags and embedded paths
# built on an Apple-silicon Mac and on arm64 Linux lay the functions out in a different order
# (Cargo mixes rustc's version string, host triple included, into every crate's symbol hashes). So
# a release builds on aarch64-unknown-linux-gnu, and only a build on that host lands on a release's
# digest; a build on a Mac is the same game with other bytes.
#
# The host tests are the gate: nothing at run time will catch a rule implemented wrongly. The
# determinism check runs first, because it is a property of the source rather than of a test.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
dist="$here/dist"
target="${CARGO_TARGET_DIR:-$here/engine/target}"

# `component new` records its own version in the component, so another wasm-tools is another digest.
WASM_TOOLS_VERSION=1.258.0

have=$(wasm-tools --version | cut -d' ' -f2)
if [ "$have" != "$WASM_TOOLS_VERSION" ]; then
  echo "warning: wasm-tools $have, not $WASM_TOOLS_VERSION -- this component's digest will not be a release's" >&2
fi

"$here/tools/deny.sh"
cd "$here/engine"
cargo test --locked

# The basic boards: every one under maps/ is what its recipe under mapgen/recipes/ makes, byte for
# byte, and obeys the rules every board obeys. A crate of its own, so tuning the generator is not an
# engine-digest change -- and since N28 regenerating a board is not one either: the component
# carries none. A season's boards are made with the same tool, outside this repository.
(cd "$here/mapgen" && cargo test --locked)

# From scratch: a board deleted from maps/ must not survive in dist/maps/, and a viewer transpiled
# from the previous component is a viewer for some other engine.
rm -rf "$dist"
mkdir -p "$dist/reference"

# THE DIGEST MUST NOT DEPEND ON WHERE THE CHECKOUT IS. rustc bakes the absolute path of every source
# file a panic can name into the binary: the crates.io sources, and -- when the rust-src component is
# installed -- the standard library's, which rustc otherwise names /rustc/<commit>. Remapping all
# three to fixed names makes the component a function of the source and the host, and the names are
# the ones the platform's releases have always carried. The variable is set, not appended to: a flag
# someone's shell exported is not part of the build.
sysroot=$(rustc --print sysroot)
commit=$(rustc -vV | sed -n 's/^commit-hash: //p')
RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}/registry/src=/cargo --remap-path-prefix=$sysroot/lib/rustlib/src/rust=/rustc/$commit --remap-path-prefix=$here=/ants" \
  cargo build --locked --release --target wasm32-unknown-unknown --lib
wasm-tools component new "$target/wasm32-unknown-unknown/release/tb_ants.wasm" -o "$dist/tb-ants.wasm"
wasm-tools validate "$dist/tb-ants.wasm" --features component-model

# The manifests are generated, because an artifact nobody hand-edits cannot drift from the code
# that produced it. The reference observations admission validates an adapter against are engine
# output for the same reason: a hand-kept copy would test a shape the game no longer produces. They
# are drawn on the basic boards, which is what makes those boards the envelope an upload must fit.
cargo run --locked --quiet --bin manifest > "$dist/cartridge.json"
cargo run --locked --quiet --bin reference > "$dist/reference/observations.json"

python3 "$here/tools/package.py"
echo "    built on        $(rustc -vV | sed -n 's/^host: //p') (a release builds on aarch64-unknown-linux-gnu)"
