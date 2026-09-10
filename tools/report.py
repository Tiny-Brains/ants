#!/usr/bin/env python3
"""What the build produced, and the digest the platform will load it under."""
import hashlib
import json
import os
import sys


def main(here):
    wasm = open(f"{here}/tb-ants.wasm", "rb").read()
    doc = json.load(open(f"{here}/reference/observations.json"))
    biggest = max(len(json.dumps(o, separators=(",", ":"))) for o in doc["observations"])
    boards = len([f for f in os.listdir(f"{here}/maps") if f.endswith(".json")])

    print()
    print("    tb-ants.wasm    %8d bytes" % len(wasm))
    print("    engine digest   sha256:%s" % hashlib.sha256(wasm).hexdigest())
    print("    boards          %8d under maps/" % boards)
    print("    reference       %8d observations, largest %d bytes"
          % (len(doc["observations"]), biggest))


if __name__ == "__main__":
    main(sys.argv[1])
