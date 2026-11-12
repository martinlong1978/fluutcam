#!/usr/bin/env bash
# Stages the dry-run build of the Flathub (source) manifest.
#
#   stage-source-build.sh <out-dir>
#
# Writes <out-dir> holding the Flathub manifest with its `git` source (a
# release tag, not yet made) replaced by the checkout this script is in, and
# cargo-sources.json. CI then runs flatpak-builder on
# <out-dir>/io.github.martinlong1978.FluutCAM.yml. By hand, with flatpak-builder:
#
#   packaging/flatpak/stage-source-build.sh work/flathub
#   flatpak-builder --user --install-deps-from=flathub --force-clean --repo=repo \
#     build work/flathub/io.github.martinlong1978.FluutCAM.yml
set -euo pipefail

out=${1:?out-dir}
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
src=$here/io.github.martinlong1978.FluutCAM.flathub.yml

grep -q '# BEGIN release-source' "$src" && grep -q '# END release-source' "$src" \
  || { echo "$src lacks the release-source markers" >&2; exit 1; }

rm -rf "$out"
mkdir -p "$out"
cp "$here/cargo-sources.json" "$out/"
awk -v root="$root" '
  /# BEGIN release-source/ { skip = 1
    print "      - type: dir"
    print "        path: " root
    print "        skip: [.git, target]"
    next }
  /# END release-source/ { skip = 0; next }
  !skip { print }
' "$src" > "$out/io.github.martinlong1978.FluutCAM.yml"
grep -q 'type: git' "$out/io.github.martinlong1978.FluutCAM.yml" \
  && { echo "a git source is left in the staged manifest" >&2; exit 1; }
echo "staged the Flathub source build in $out"
