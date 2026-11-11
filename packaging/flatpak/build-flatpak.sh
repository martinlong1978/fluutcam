#!/usr/bin/env bash
# Stages the inputs of the Flatpak build from the Linux release tarball
# (D14 section 5.2; plan docs/plan/2026-11-11-flatpak.md).
#
#   build-flatpak.sh <tarball> <version> <out-dir>
#
# Writes <out-dir> holding the manifest, the launcher, the metainfo and
# payload/ (the unpacked tarball). The workflows then run flatpak-builder on
# <out-dir>/io.github.martinlong1978.FluutCAM.yml. By hand, with flatpak-builder installed:
#
#   flatpak-builder --user --install-deps-from=flathub --force-clean --repo=repo build \
#     <out-dir>/io.github.martinlong1978.FluutCAM.yml
#   flatpak build-bundle repo fluutcam.flatpak io.github.martinlong1978.FluutCAM
set -euo pipefail

tarball=${1:?tarball}
version=${2:?version}
out=${3:?out-dir}

here=$(cd "$(dirname "$0")" && pwd)
id=io.github.martinlong1978.FluutCAM

rm -rf "$out"
mkdir -p "$out/payload"
tar -xzf "$tarball" -C "$out/payload" --strip-components=1
for f in bin/fluutcam bin/fluut share/doc/fluutcam/LICENSE-APACHE share/doc/fluutcam/LICENSE-MIT \
         share/icons/hicolor/128x128/apps/fluutcam.png share/icons/hicolor/512x512/apps/fluutcam.png; do
  [ -f "$out/payload/$f" ] || { echo "tarball lacks $f" >&2; exit 1; }
done
chmod 755 "$out/payload/bin/fluutcam" "$out/payload/bin/fluut"
cp "$here/$id.yml" "$here/$id.desktop" "$here/$id.metainfo.xml" "$out/"

# The metainfo must have a release entry for this version, or the software
# centres show an old one. A pre-release (1.2.3-rc.1) only gets a warning.
if ! grep -q "<release version=\"$version\"" "$out/$id.metainfo.xml"; then
  case "$version" in
    *-*) echo "::warning::$id.metainfo.xml has no <release> for $version" ;;
    *) echo "::error::$id.metainfo.xml has no <release> for $version; add one" >&2; exit 1 ;;
  esac
fi
echo "staged the Flatpak inputs in $out"
