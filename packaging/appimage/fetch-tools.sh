#!/usr/bin/env bash
# Downloads appimagetool and the AppImage runtime into <dir>, each pinned to an
# exact release and checked against a SHA-256 written here. A mismatch stops the
# build. To update: read the new release's asset digest on GitHub, review the
# release, change the version and hash together.
#
#   fetch-tools.sh <dir>     (writes <dir>/appimagetool and <dir>/runtime-x86_64)
set -euo pipefail

TOOL_VERSION=1.9.1
TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

dir=${1:?dir}
mkdir -p "$dir"
fetch() { # url file sha256
  curl -fsSL --retry 3 -o "$2" "$1"
  echo "$3  $2" | sha256sum -c -
}
fetch "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-x86_64.AppImage" \
  "$dir/appimagetool" "$TOOL_SHA256"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64" \
  "$dir/runtime-x86_64" "$RUNTIME_SHA256"
chmod +x "$dir/appimagetool"
