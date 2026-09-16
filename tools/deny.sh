#!/bin/sh
# The determinism law, checked mechanically: no floating point in game logic.
#
# It is what makes the platform's build and the browser's agree bit for bit, which is what lets a
# replay be an action stream rather than a hundred times as many frames. Nothing in Rust enforces
# it, and a single `as f64` in a loop bound would be invisible until two builds disagreed on a match.
#
# engine/src/ only, and not its tests/ or bin/: those are host tooling and never reach the component.
set -eu
src=$(cd "$(dirname "$0")/../engine/src" && pwd)
bad=$(grep -rnE '\bf32\b|\bf64\b|\.sqrt\(|\.powi\(|\.powf\(|[0-9]\.[0-9]' \
        "$src" --include='*.rs' --exclude-dir=tests --exclude-dir=bin | grep -v '://' || true)
if [ -n "$bad" ]; then
  echo "floating point in game logic -- the determinism law forbids it:" >&2
  echo "$bad" >&2
  exit 1
fi
echo "determinism: no floating point in game logic"
