#!/bin/sh
# The determinism law, checked mechanically -- docs/docs/cartridge.md §4 and the platform design §3.3.
#
# "Game state must be integer-only. No floating point in game logic." It is what makes the
# platform's build and the browser's agree bit for bit, which is what lets a replay be an action
# stream rather than a hundred times as many frames. Nothing in Rust enforces it, and a single
# `as f64` in a loop bound would be invisible until two builds disagreed on a match.
#
# So: no float types, no float literals, no float-returning maths, anywhere in the game logic.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bad=$(grep -nE '\bf32\b|\bf64\b|\.sqrt\(|\.powi\(|\.powf\(|[0-9]\.[0-9]' \
        "$here/src"/*.rs | grep -v '^.*://' | grep -v 'src/tests.rs' || true)
if [ -n "$bad" ]; then
  echo "floating point in game logic -- docs/docs/cartridge.md §4 forbids it:" >&2
  echo "$bad" >&2
  exit 1
fi
echo "determinism: no floating point in game logic"
