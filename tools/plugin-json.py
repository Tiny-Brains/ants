#!/usr/bin/env python3
"""plugin.toml -> plugin.json.

plugin.toml is the authored form: it is what `orion-cli plugins create -f` reads. The admin API
takes JSON, and the load script runs inside an image with jq and no TOML parser, so the JSON is a
generated artifact exactly like the component beside it -- never hand-edited.
"""
import json
import sys
import tomllib


def main(here):
    with open(f"{here}/plugin.toml", "rb") as f:
        manifest = tomllib.load(f)
    with open(f"{here}/plugin.json", "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    print("    plugin.json: %d functions" % len(manifest.get("functions", [])))


if __name__ == "__main__":
    main(sys.argv[1])
