#!/usr/bin/env bash
# Checks the launcher and the AppStream metainfo of the Flatpak. Run by the
# workflows before the build, so a bad file stops it with a clear message.
#
#   validate.sh [dir]     (default: the folder of this script)
#
# Needs appstreamcli (package `appstream`) and desktop-file-validate
# (`desktop-file-utils`); the CI container has them or gets them from its
# package manager.
set -euo pipefail

dir=${1:-$(cd "$(dirname "$0")" && pwd)}
id=io.github.martinlong1978.FluutCAM

need() { # command package
  command -v "$1" >/dev/null && return 0
  if command -v dnf >/dev/null; then dnf -y install "$2"
  elif command -v apt-get >/dev/null; then
    ${SUDO:-sudo} apt-get update && ${SUDO:-sudo} apt-get install -y "$2"
  fi
  command -v "$1" >/dev/null || { echo "$1 is not installed (package $2)" >&2; exit 1; }
}
need appstreamcli appstream
need desktop-file-validate desktop-file-utils

desktop-file-validate "$dir/$id.desktop"
# --no-net: the screenshot addresses point at the main branch, which a pull
# request does not change; the check must not depend on the network.
appstreamcli validate --no-net "$dir/$id.metainfo.xml"
echo "validated $id.desktop and $id.metainfo.xml"
