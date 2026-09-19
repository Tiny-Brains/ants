#!/usr/bin/env python3
"""Finish dist/: what the Rust build cannot write, and a report of what it all adds up to.

build.sh has already put the component, the generated half of cartridge.json and the reference
observations in dist/. This adds the rest, so that dist/ is exactly the tree a release archive
carries -- a checkout and a downloaded release are read the same way.

  plugin.toml     copied. It is the authored ABI, and what `orion-server compile` reads.
  plugin.json     plugin.toml as JSON: the loader runs in an image with jq and no TOML parser.

  cartridge.json  gains `maps`, `limits.boards` and `about`.
                  `maps` is metadata and a digest per basic board, not the boards: the document is
                  read once at registration and stored on the game row, so it carries what a caller
                  needs to CHOOSE and CHECK a board while the boards travel as files. The digest is
                  over the file exactly as committed, so a competitor with a stale export is told so.
                  `limits.boards` is THE ENVELOPE: the seat counts, sides and cells the basic
                  boards span, which is what the reference set is drawn on and so what admission
                  has proved an adapter against. A season's upload must fit inside it -- Soma
                  refuses one that does not -- because maps change while a season is live and a
                  model admitted today must play the board added next week.
                  `about` is the one hand-written input (engine/about.json), here rather than in the
                  website so a second cartridge can introduce itself without a web deploy. Every
                  value is plain text: a manifest that could carry markup could carry a script tag.

  maps/           the basic boards themselves, as committed. Never a season's: those are uploaded
                  to the platform and are in no repository and no release.
"""
import hashlib
import json
import os
import shutil
import sys
import tomllib

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ENGINE = os.path.join(HERE, "engine")
DIST = os.path.join(HERE, "dist")
MAPS = os.path.join(HERE, "maps")


def plugin():
    shutil.copyfile(os.path.join(ENGINE, "plugin.toml"), os.path.join(DIST, "plugin.toml"))
    with open(os.path.join(ENGINE, "plugin.toml"), "rb") as f:
        manifest = tomllib.load(f)
    with open(os.path.join(DIST, "plugin.json"), "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    print("    plugin.json: %d functions" % len(manifest.get("functions", [])))


def boards():
    return sorted(n for n in os.listdir(MAPS) if n.endswith(".json"))


def catalogue():
    out = []
    for name in boards():
        raw = open(os.path.join(MAPS, name), "rb").read()
        m = json.loads(raw)
        if m["id"] != name[:-5]:
            sys.exit("%s declares id %r; a board is named by its file" % (name, m["id"]))
        out.append({
            "id": m["id"],
            "rows": m["rows"],
            "cols": m["cols"],
            "players": m["players"],
            "food_target": m["food_target"],
            "sha256": "sha256:" + hashlib.sha256(raw).hexdigest(),
        })
    return out


def envelope(maps):
    """What a season's upload may be: whatever the basic boards span, and nothing they do not.

    Three bounds, because an adapter is proved on the boards the reference set was drawn on and a
    board outside them is a board nobody has proved it against: the seat counts (every owner number
    an adapter must encode), each side (the smallest and largest a reshape must handle), and the
    cells (what the ops budget scales with -- a square board at the largest side is more cells than
    any basic board, so the side alone would let one through).
    """
    players = [m["players"] for m in maps]
    sides = [s for m in maps for s in (m["rows"], m["cols"])]
    return {
        "players": [min(players), max(players)],
        "sides": [min(sides), max(sides)],
        "cells_max": max(m["rows"] * m["cols"] for m in maps),
    }


def about():
    src = json.load(open(os.path.join(ENGINE, "about.json")))

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


def cartridge():
    path = os.path.join(DIST, "cartridge.json")
    doc = json.load(open(path))
    doc["maps"] = catalogue()
    doc["about"] = about()

    if not doc["maps"]:
        sys.exit("no boards under maps/ -- make them with `cd mapgen && cargo run -- generate`")
    doc["limits"]["boards"] = envelope(doc["maps"])

    with open(path, "w") as f:
        json.dump(doc, f, indent=2, sort_keys=True)
        f.write("\n")
    b = doc["limits"]["boards"]
    print("    cartridge.json: %d basic boards; uploads %d-%d seats, sides %d-%d, at most %d cells; "
          "%d about paragraphs" % (len(doc["maps"]), b["players"][0], b["players"][1],
                                   b["sides"][0], b["sides"][1], b["cells_max"],
                                   len(doc["about"]["story"])))


def copy_boards():
    os.makedirs(os.path.join(DIST, "maps"), exist_ok=True)
    for name in boards():
        shutil.copyfile(os.path.join(MAPS, name), os.path.join(DIST, "maps", name))


def report():
    wasm = open(os.path.join(DIST, "tb-ants.wasm"), "rb").read()
    doc = json.load(open(os.path.join(DIST, "reference", "observations.json")))
    biggest = max(len(json.dumps(o, separators=(",", ":"))) for o in doc["observations"])

    print()
    print("    tb-ants.wasm    %8d bytes" % len(wasm))
    print("    engine digest   sha256:%s" % hashlib.sha256(wasm).hexdigest())
    print("    boards          %8d under maps/" % len(boards()))
    print("    reference       %8d observations, largest %d bytes"
          % (len(doc["observations"]), biggest))
    print("    ==> dist/")


if __name__ == "__main__":
    plugin()
    cartridge()
    copy_boards()
    report()
