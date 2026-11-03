#!/usr/bin/env bash
# Builds fluutcam-<version>-linux-x86_64.AppImage from the Linux release tarball
# (D14 section 5.2; plan docs/plan/2026-11-03-msi-appimage.md).
#
#   build-appimage.sh <tarball> <version> <out-dir>
#
# Environment:
#   APPIMAGETOOL  path of the appimagetool executable (pinned and checked by release.yml).
#                 If unset, only the AppDir is assembled (<out-dir>/AppDir), for testing.
#   RUNTIME       path of the pinned type2 runtime file; appimagetool would otherwise
#                 download one without a checksum.
#   SOURCE_DATE_EPOCH  honoured by mksquashfs, for a repeatable image.
set -euo pipefail

tarball=${1:?tarball}
version=${2:?version}
out=${3:?out-dir}

mkdir -p "$out"
appdir="$out/AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr"

# The tarball has one top folder, fluutcam-<version>/, with bin/ and share/.
tar -xzf "$tarball" -C "$appdir/usr" --strip-components=1
for f in usr/bin/fluutcam usr/bin/fluut usr/share/applications/fluutcam.desktop \
         usr/share/icons/hicolor/256x256/apps/fluutcam.png; do
  [ -f "$appdir/$f" ] || { echo "tarball lacks $f" >&2; exit 1; }
done

# appimagetool wants the launcher and the icon at the top of the AppDir.
cp "$appdir/usr/share/applications/fluutcam.desktop" "$appdir/fluutcam.desktop"
cp "$appdir/usr/share/icons/hicolor/256x256/apps/fluutcam.png" "$appdir/fluutcam.png"
ln -s fluutcam.png "$appdir/.DirIcon"

# AppRun starts the application. Run through a link named `fluut` (or with
# ARGV0 ending in `fluut`) it starts the command line program instead.
cat > "$appdir/AppRun" <<'RUN'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
case "$(basename "${ARGV0:-fluutcam}")" in
  fluut) exec "$here/usr/bin/fluut" "$@" ;;
  *) exec "$here/usr/bin/fluutcam" "$@" ;;
esac
RUN
chmod 755 "$appdir/AppRun" "$appdir/usr/bin/fluutcam" "$appdir/usr/bin/fluut"

if [ -z "${APPIMAGETOOL:-}" ]; then
  echo "APPIMAGETOOL not set: AppDir assembled in $appdir"
  exit 0
fi

args=(--appimage-extract-and-run)
image="$out/fluutcam-$version-linux-x86_64.AppImage"
[ -z "${RUNTIME:-}" ] || args+=(--runtime-file "$RUNTIME")
ARCH=x86_64 "$APPIMAGETOOL" "${args[@]}" --no-appstream "$appdir" "$image"
chmod 755 "$image"
echo "wrote $image"
