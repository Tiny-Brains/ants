#!/usr/bin/env python3
"""Fold the map catalogue into cartridge.json.

Metadata and a digest per board, not the boards themselves: this document is read once at
registration and stored on the game row, so it carries what a caller needs to CHOOSE and CHECK a
board while the boards travel as files.

The digest is over the file exactly as committed, so a competitor who exported the catalogue into
their own checkout can be told their copy is stale rather than quietly playing a different board
from the one the engine plays.
"""
import hashlib
import json
import os
import sys


def main(here):
    mapdir = os.path.join(here, "maps")
    catalogue = []
    for name in sorted(os.listdir(mapdir)):
        if not name.endswith(".json"):
            continue
        raw = open(os.path.join(mapdir, name), "rb").read()
        m = json.loads(raw)
        if m["id"] != name[:-5]:
            sys.exit("%s declares id %r; a board is named by its file" % (name, m["id"]))
        catalogue.append({
            "id": m["id"],
            "preset": m["preset"],
            "rows": m["rows"],
            "cols": m["cols"],
            "players": m["players"],
            "food_target": m["food_target"],
            "sha256": "sha256:" + hashlib.sha256(raw).hexdigest(),
        })

    path = os.path.join(here, "cartridge.json")
    doc = json.load(open(path))
    doc["maps"] = catalogue

    counts = {}
    for entry in catalogue:
        counts[entry["preset"]] = counts.get(entry["preset"], 0) + 1
    for preset in doc["presets"]:
        preset["maps"] = counts.get(preset["name"], 0)
        # A preset nothing is played on would pair matches that worldgen then refuses, per match,
        # in production -- the exact failure generating this manifest exists to prevent.
        if not preset["maps"]:
            sys.exit("preset %r is played on no board" % preset["name"])

    with open(path, "w") as f:
        json.dump(doc, f, indent=2, sort_keys=True)
        f.write("\n")
    print("    catalogue: %d boards across %d presets" % (len(catalogue), len(counts)))


if __name__ == "__main__":
    main(sys.argv[1])
