#!/bin/sh
# Rebuild the tb.ants component and every artifact that ships beside it.
#
# Needs the wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`), `wasm-tools`
# (`cargo install wasm-tools`) and Python 3.11 or newer. Run it after changing src/ or maps/.
#
# THE OUTPUT IS NOT COMMITTED, AND WHAT THIS WRITES IS NOT WHAT SHIPS. The artifact every consumer
# loads is built by ../Dockerfile, which runs this script under a pinned toolchain with build paths
# remapped -- so the component's digest is a function of the source rather than of the machine.
# This copy is for working here: the tests, the manifests, and a viewer you can open. When you want
# the bytes the platform will actually run, build the image.
#
# The host tests are the gate. docs/protocol.md §6: the game's own schemas are documentation the
# platform never loads, so nothing at run time will catch a rule implemented wrongly. `deny.sh` runs
# first, because the determinism law is a property of the source rather than of a test.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

./deny.sh

# The boards, compiled in. Regenerated before the tests, because the corpus test can only see the
# maps this step embedded.
python3 tools/embed-maps.py "$here"

cargo test
cargo build --release --target wasm32-unknown-unknown --lib
wasm-tools component new target/wasm32-unknown-unknown/release/tb_ants.wasm -o tb-ants.wasm
wasm-tools validate tb-ants.wasm --features component-model

# The manifests. Every one is generated, because an artifact nobody hand-edits cannot drift from
# the code that produced it.
python3 tools/plugin-json.py "$here"
cargo run --quiet --bin manifest > cartridge.json
python3 tools/cartridge.py "$here"

# The reference observation set admission validates an adapter against. Generated for the same
# reason: it is ENGINE OUTPUT, so a hand-maintained copy would drift from the payloads a model
# actually meets, and the gate would be testing a shape the game no longer produces.
cargo run --quiet --bin reference > reference/observations.json

# The schemas, if jsonschema is installed. They are documentation the platform never loads, so this
# is not a hard dependency of the build -- but a schema that has drifted from the manifest is worse
# than none, and that is exactly what it had done.
if python3 -c 'import jsonschema' 2> /dev/null; then
  python3 schema/validate.py | tail -1 | sed 's/^/    schemas: /'
else
  echo "    schemas: skipped (pip install jsonschema)"
fi

python3 tools/report.py "$here"
