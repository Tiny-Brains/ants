#!/bin/sh
# The determinism law, checked mechanically -- docs/cartridge.md §4.
#
# "Game state must be integer-only. No floating point in game logic." It is what makes the
# platform's build and the browser's agree bit for bit, which is what lets a replay be an action
# stream rather than a hundred times as many frames. Nothing in Rust enforces it, and a single
# `as f64` in a loop bound would be invisible until two builds disagreed on a match.
#
# src/ only: the tests and src/bin/ are host tooling, and the manifest generator publishes FLOP caps
# that are floats by definition.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bad=$(grep -rnE '\bf32\b|\bf64\b|\.sqrt\(|\.powi\(|\.powf\(|[0-9]\.[0-9]' \
        "$here/src" --include='*.rs' --exclude-dir=tests --exclude-dir=bin | grep -v '://' || true)
if [ -n "$bad" ]; then
  echo "floating point in game logic -- docs/cartridge.md §4 forbids it:" >&2
  echo "$bad" >&2
  exit 1
fi
echo "determinism: no floating point in game logic"
