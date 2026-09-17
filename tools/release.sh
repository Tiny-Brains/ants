#!/bin/sh
# The artifact set as ONE archive on a GitHub release, so a games registry can pin it and nobody
# who plays Ants needs this repository beside theirs.
#
#   tools/release.sh              build the image, pack /artifacts/, print what a registry pins
#   tools/release.sh --publish    ...and create the release (needs `gh`, a clean tree, HEAD pushed)
#
# THE ARCHIVE IS THE IMAGE'S /artifacts/, never dist/: only the image pins rustc and remaps build
# paths, so only its digest is the one the ladder plays. It is built here under its own tag so a
# release never moves `tinybrains/ants:dev` under a stack that is running on it.
#
# ONE ARCHIVE, NOT A FILE PER ARTIFACT, because every consumer reads a tree: `tinybrains check`
# wants reference/, `view` wants viz/, `maps export` wants maps/. It is packed deterministically --
# sorted names, no timestamps, no owners -- so packing the same image twice is the same bytes.
#
# THE TAG IS THE ENGINE. `engine-<12 hex>` says which component a release carries, so there is no
# version number to keep in step with a digest, and a release is never re-cut under an old tag: a
# registry pins the archive's digest, and replacing the file would break every one that does.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
repo=Tiny-Brains/ants
archive_name=ants-artifacts.tar.gz

publish=0
case "${1:-}" in
  "") ;;
  --publish) publish=1 ;;
  *) echo "usage: tools/release.sh [--publish]" >&2; exit 2 ;;
esac

cd "$here"
if [ "$publish" = 1 ]; then
  if [ -n "$(git status --porcelain)" ]; then
    echo "refusing to publish from a dirty tree: the release would name a commit that did not build it" >&2
    exit 1
  fi
  git fetch -q origin
  if ! git merge-base --is-ancestor HEAD origin/main; then
    echo "refusing to publish: HEAD is not on origin/main, so the tag would point at nothing public" >&2
    exit 1
  fi
fi

out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT

docker build -q -t tinybrains/ants:release . >/dev/null
id=$(docker create tinybrains/ants:release)
docker cp "$id":/artifacts/. "$out/artifacts" >/dev/null
docker rm "$id" >/dev/null

python3 - "$out/artifacts" "$out/$archive_name" > "$out/digests" <<'EOF'
import gzip, hashlib, io, os, sys, tarfile

src, dst = sys.argv[1], sys.argv[2]
names = sorted(
    os.path.relpath(os.path.join(d, f), src).replace(os.sep, "/")
    for d, _, files in os.walk(src)
    for f in files
)
raw = io.BytesIO()
with tarfile.open(fileobj=raw, mode="w", format=tarfile.USTAR_FORMAT) as tar:
    for name in names:
        with open(os.path.join(src, name), "rb") as f:
            data = f.read()
        info = tarfile.TarInfo(name)
        info.size, info.mode, info.mtime = len(data), 0o644, 0
        tar.addfile(info, io.BytesIO(data))
with open(dst, "wb") as f, gzip.GzipFile(filename="", mode="wb", fileobj=f, mtime=0) as gz:
    gz.write(raw.getvalue())

component = [n for n in names if "/" not in n and n.endswith(".wasm")]
if len(component) != 1:
    sys.exit(f"expected one component at the root of /artifacts/, found {component}")
with open(os.path.join(src, component[0]), "rb") as f:
    print("sha256:" + hashlib.sha256(f.read()).hexdigest())
with open(dst, "rb") as f:
    print("sha256:" + hashlib.sha256(f.read()).hexdigest())
EOF
engine=$(sed -n 1p "$out/digests")
archive=$(sed -n 2p "$out/digests")
tag="engine-$(echo "$engine" | cut -c8-19)"

block=$(cat <<EOF
[games.ants]
name = "Ants"
repo = "$repo"
release = "$tag"
artifacts = { file = "$archive_name", sha256 = "$archive" }
engine = "$engine"
EOF
)

echo "engine   $engine"
echo "archive  $archive  ($(wc -c < "$out/$archive_name" | tr -d ' ') bytes)"
echo "tag      $tag"
echo
echo "$block"

[ "$publish" = 1 ] || exit 0

if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  echo "$tag is already released; a release is never replaced, because registries pin its archive" >&2
  exit 1
fi
gh release create "$tag" "$out/$archive_name" --repo "$repo" --target "$(git rev-parse HEAD)" \
  --title "Ants engine ${engine}" --notes "$(cat <<EOF
The Ants cartridge's artifact set -- the component, \`cartridge.json\`, the boards, the reference
observations and the replay viewer -- exactly as the artifact image carries it under \`/artifacts/\`.

A games registry pins it:

\`\`\`toml
$block
\`\`\`

\`tinybrains\` downloads it once, refuses it unless the archive and the component inside it hash to
those two digests, and unpacks it under its cache.
EOF
)"
