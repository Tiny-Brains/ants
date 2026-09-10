#!/usr/bin/env python3
"""Fold the board catalogue and about.json into cartridge.json.

`cargo run --bin manifest` writes the generated half -- presets, limits, budgets. This adds the two
things it cannot know:

  maps    Metadata and a digest per board, not the boards themselves: the document is read once at
          registration and stored on the game row, so it carries what a caller needs to CHOOSE and
          CHECK a board while the boards travel as files. The digest is over the file exactly as
          committed, so a competitor with a stale export is told so rather than quietly playing a
          different board from the one the engine plays.

  about   The one hand-written input. It is here rather than in the website because a second
          cartridge must be able to introduce itself without a web deploy. Keys not named below are
          dropped and every value is plain text: this document is registered from a repository and
          rendered in a browser, and a manifest that could carry markup could carry a script tag.
"""
import hashlib
import json
import os
import sys


def catalogue(here):
    mapdir = os.path.join(here, "maps")
    out = []
    for name in sorted(os.listdir(mapdir)):
        if not name.endswith(".json"):
            continue
        raw = open(os.path.join(mapdir, name), "rb").read()
        m = json.loads(raw)
        if m["id"] != name[:-5]:
            sys.exit("%s declares id %r; a board is named by its file" % (name, m["id"]))
        out.append({
            "id": m["id"],
            "preset": m["preset"],
            "rows": m["rows"],
            "cols": m["cols"],
            "players": m["players"],
            "food_target": m["food_target"],
            "sha256": "sha256:" + hashlib.sha256(raw).hexdigest(),
        })
    return out


def about(here):
    src = json.load(open(f"{here}/about.json"))

    def text(v):
        if not isinstance(v, str):
            sys.exit("about.json: every value must be a string, got %r" % (v,))
        return v

    doc = {
        "tagline": text(src["tagline"]),
        "provenance": text(src["provenance"]),
        "story": [text(p) for p in src["story"]],
        "links": [{"label": text(l["label"]), "href": text(l["href"])} for l in src["links"]],
    }
    for link in doc["links"]:
        if not link["href"].startswith("https://"):
            sys.exit("about.json: link %r must be https" % link["href"])
    return doc


def main(here):
    path = os.path.join(here, "cartridge.json")
    doc = json.load(open(path))
    doc["maps"] = catalogue(here)
    doc["about"] = about(here)

    counts = {}
    for entry in doc["maps"]:
        counts[entry["preset"]] = counts.get(entry["preset"], 0) + 1
    for preset in doc["presets"]:
        preset["maps"] = counts.get(preset["name"], 0)
        # A preset played on no board would pair matches that worldgen then refuses, per match, in
        # production -- the exact failure generating this manifest exists to prevent.
        if not preset["maps"]:
            sys.exit("preset %r is played on no board" % preset["name"])

    with open(path, "w") as f:
        json.dump(doc, f, indent=2, sort_keys=True)
        f.write("\n")
    print("    cartridge.json: %d boards across %d presets, %d about paragraphs"
          % (len(doc["maps"]), len(counts), len(doc["about"]["story"])))


if __name__ == "__main__":
    main(sys.argv[1])
