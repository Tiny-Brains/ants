#!/usr/bin/env python3
"""Pack dist/ into the one archive a release carries, and say what it is.

    tools/pack.py <out-dir>

Writes <out-dir>/ants-artifacts.tar.gz and prints the engine digest, the archive's digest, its
content digest and the tag a release of it takes. With GITHUB_OUTPUT set, the same values go there
as `engine`, `archive`, `content` and `hex`.

ONE ARCHIVE, NOT A FILE PER ARTIFACT, because every consumer reads a tree: `tinybrains check` wants
reference/, `view` wants viz/, kalam wants the component beside its manifests. It is packed
deterministically -- sorted names, no timestamps, no owners -- so the same dist/ is the same bytes.

THE CONTENT DIGEST is the sha256 of the tar before gzip. The archive's own digest is what a registry
pins; the content digest is what says two builds made the same thing, whatever zlib compressed them.

A COMPLETE SET OR NOTHING. A release without the viewer, or with a viewer transpiled from another
component, would be pinned by every registry that takes it, so both are refused here rather than
discovered by a page that draws a match that never happened.
"""
import gzip
import hashlib
import io
import json
import os
import sys
import tarfile

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIST = os.path.join(HERE, "dist")
ARCHIVE = "ants-artifacts.tar.gz"
REQUIRED = [
    "tb-ants.wasm", "plugin.toml", "plugin.json", "cartridge.json",
    "reference/observations.json", "viz/viz.js", "viz/engine.json",
]


def sha256(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: tools/pack.py <out-dir>")
    out = sys.argv[1]

    missing = [p for p in REQUIRED if not os.path.isfile(os.path.join(DIST, p))]
    if missing:
        sys.exit("dist/ is missing %s -- run ./build.sh, then viz/build.sh" % ", ".join(missing))
    names = sorted(
        os.path.relpath(os.path.join(d, f), DIST).replace(os.sep, "/")
        for d, _, files in os.walk(DIST)
        for f in files
    )
    if not any(n.startswith("maps/") for n in names):
        sys.exit("dist/maps/ is empty -- a cartridge with no boards plays nothing")
    wasm = [n for n in names if "/" not in n and n.endswith(".wasm")]
    if wasm != ["tb-ants.wasm"]:
        sys.exit("expected one component at the root of dist/, found %s" % wasm)

    engine = sha256(open(os.path.join(DIST, "tb-ants.wasm"), "rb").read())
    viewer = json.load(open(os.path.join(DIST, "viz", "engine.json")))["engine_digest"]
    if viewer != engine:
        sys.exit("dist/viz/ was transpiled from %s, not this component (%s) -- run viz/build.sh"
                 % (viewer, engine))

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        for name in names:
            with open(os.path.join(DIST, name), "rb") as f:
                data = f.read()
            info = tarfile.TarInfo(name)
            info.size, info.mode, info.mtime = len(data), 0o644, 0
            tar.addfile(info, io.BytesIO(data))
    content = sha256(raw.getvalue())

    os.makedirs(out, exist_ok=True)
    path = os.path.join(out, ARCHIVE)
    with open(path, "wb") as f, gzip.GzipFile(filename="", mode="wb", fileobj=f, mtime=0) as gz:
        gz.write(raw.getvalue())
    archive = sha256(open(path, "rb").read())
    hex12 = engine[len("sha256:"):][:12]

    print("engine   %s" % engine)
    print("archive  %s  (%d bytes, %d files)" % (archive, os.path.getsize(path), len(names)))
    print("content  %s" % content)
    print("tag      engine-%s" % hex12)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as f:
            f.write("engine=%s\narchive=%s\ncontent=%s\nhex=%s\n" % (engine, archive, content, hex12))


if __name__ == "__main__":
    main()
