#!/bin/sh
# Rebuild the tb.ants component and place it beside the manifest that names it.
#
# Needs the wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`) and
# `wasm-tools` (`cargo install wasm-tools`). The output is committed, so the package loads on a
# machine with no wasm toolchain; run this after changing src/ and commit the result with it.
#
# The host tests are the gate. RULES.md is the specification and `src/tests.rs` is the only
# place it is enforced -- the game's own schemas are documentation the platform never loads
# (PROTOCOL.md §6), so nothing at runtime will catch a rule implemented wrongly. `deny.sh` runs
# first: the determinism law is a property of the source, not of a test.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
"$here/deny.sh"
cd "$here"

# The boards, compiled in. A cartridge imports nothing -- no filesystem (docs/cartridge.md §4) --
# so a map cannot be read at run time and every committed board is embedded instead. That is also
# what makes a map edit an engine-digest change, and therefore refused while a season is live, on
# the same rails a rules change already runs on.
#
# Regenerated before the tests, because `every_committed_map_is_valid_and_symmetric` is what
# validates the corpus and it can only see the maps this step embedded.
python3 "$here/tools/embed-maps.py" "$here"

cargo test
cargo build --release --target wasm32-unknown-unknown
wasm-tools component new \
  target/wasm32-unknown-unknown/release/tb_ants.wasm \
  -o "$here/tb-ants.wasm"
wasm-tools validate "$here/tb-ants.wasm" --features component-model

# The manifest, again as JSON. `plugin.toml` is the authored form -- it is what `orion-cli plugins
# create -f` reads, and what Orion's own examples use -- but the admin API takes JSON, and the
# load script runs inside the Soma image, which has jq and base64 and no TOML parser. So the JSON
# is a generated, committed artifact exactly like the component beside it: never hand-edited,
# always rebuilt from the TOML by this script.
python3 - "$here/plugin.toml" "$here/plugin.json" <<'PYEOF'
import json, sys, tomllib
src, dst = sys.argv[1], sys.argv[2]
with open(src, "rb") as f:
    manifest = tomllib.load(f)
with open(dst, "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
PYEOF

# The registration manifest, generated from the preset table for the same reason the plugin
# manifest is generated from the TOML: an artifact nobody hand-edits cannot drift from the code.
cargo run --quiet --bin manifest > "$here/cartridge.json"

# The map catalogue, folded into that same manifest -- metadata and a digest per board, not the
# boards themselves. This document is read once at registration and stored on the game row, so it
# carries what a caller needs to CHOOSE and CHECK a board while the boards travel as files.
python3 "$here/tools/catalogue.py" "$here"

# What the game says about itself, folded into the same manifest. THE ONE HAND-WRITTEN INPUT: the
# rest of cartridge.json is generated so it cannot drift from the code, and this has no code to
# drift from. It is here rather than in the website because a second cartridge must be able to
# introduce itself without a web deploy -- the home page's provenance section reads `about` and
# renders whatever the selected game published. Keys the schema below does not name are dropped, and
# every value is plain text: this document is registered from a repository and rendered in a
# browser, and a manifest that could carry markup is a manifest that could carry a script tag.
#
# FOUR KEYS AND NO SHAPES. An earlier draft also folded in a comparison table -- before/after rows
# against two era labels -- and it was dropped, because that structure only means anything for a
# game that is a remake of something. A second cartridge would ship an empty one or bend its own
# story to fit a frame built for this one. Prose says whatever a game needs; a schema cannot.
#
# about.json itself carries no commentary -- it is the copy and nothing else, so that what a
# cartridge author writes is what a reader sees. Why it has the shape it has is here, beside the
# code that enforces the shape.
python3 - "$here" <<'ABOUTEOF'
import json, sys
here = sys.argv[1]
cart = json.load(open(f"{here}/cartridge.json"))
src = json.load(open(f"{here}/about.json"))

def text(v):
    if not isinstance(v, str):
        raise SystemExit("about.json: every value must be a string, got %r" % (v,))
    return v

about = {
    "tagline":    text(src["tagline"]),
    "provenance": text(src["provenance"]),
    "story":      [text(p) for p in src["story"]],
    "links":      [{"label": text(l["label"]), "href": text(l["href"])} for l in src["links"]],
}
for l in about["links"]:
    if not l["href"].startswith("https://"):
        raise SystemExit("about.json: link %r must be https" % l["href"])

cart["about"] = about
json.dump(cart, open(f"{here}/cartridge.json", "w"), indent=2, sort_keys=True)
open(f"{here}/cartridge.json", "a").write("\n")
print("    about: %d paragraphs, %d links" % (len(about["story"]), len(about["links"])))
ABOUTEOF

# The reference observation set admission validates against. Generated for the same reason the
# manifests are: it is ENGINE OUTPUT, so a hand-maintained copy would drift from the payloads a
# model actually meets, and the gate would be testing a shape the game no longer produces.
#
# What is not in this file is not checked, so it is a spread -- every preset, sparse and crowded --
# rather than one mid-game board. `src/bin/reference.rs` says what it samples and why.
cargo run --quiet --bin reference > "$here/reference/observations.json"
python3 - "$here" <<'REFEOF'
import json, sys
doc = json.load(open(sys.argv[1] + "/reference/observations.json"))
obs = doc["observations"]
biggest = max(len(json.dumps(o, separators=(",", ":"))) for o in obs)
print("    reference: %d observations, largest %d bytes" % (len(obs), biggest))
REFEOF

ls -l "$here/tb-ants.wasm" "$here/plugin.json" "$here/cartridge.json"
ls -d "$here/maps" | sed "s/^/    boards: /"
