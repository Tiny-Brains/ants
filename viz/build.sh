#!/bin/sh
# Build the viewer into viz/dist, which is committed exactly as tb-ants.wasm is.
#
# Two steps and no bundler. `jco transpile` turns the component into an ES module the browser can
# import -- so the browser re-simulates through THE SAME CARTRIDGE the referee used, which is the
# whole reason the determinism law exists. The rest of the viewer is already ES modules, so it is
# copied rather than compiled.
#
# Committing dist/ is what keeps Node off everyone else's critical path: only someone changing the
# viewer needs it, exactly as only someone changing the engine needs the wasm toolchain.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

wasm="${1:-$here/../tb-ants.wasm}"
[ -r "$wasm" ] || { echo "no component at $wasm -- run ../build.sh first" >&2; exit 1; }

rm -rf dist
mkdir -p dist/engine
npx --yes @bytecodealliance/jco@1.32.1 transpile "$wasm" -o dist/engine --name tb-ants > /dev/null
cp src/*.js dist/
mv dist/index.js dist/viz.js

# The digest of the component the viewer carries. It must equal the engine digest a replay names,
# or the viewer is re-simulating a match some other engine played -- which is exactly the failure
# that makes a viewer worth less than no viewer.
if command -v sha256sum > /dev/null 2>&1; then d=$(sha256sum "$wasm" | cut -d' ' -f1)
else d=$(shasum -a 256 "$wasm" | cut -d' ' -f1); fi
cat > dist/engine.json <<JSON
{
  "engine_digest": "sha256:$d",
  "note": "The component this viewer re-simulates with. A replay naming a different engine_digest is a replay this viewer cannot faithfully show."
}
JSON

echo "==> viz/dist"
ls -l dist/*.js dist/engine.json | sed 's/^/    /'
ls -l dist/engine/*.wasm | sed 's/^/    /'
echo "    engine sha256:$d"
