#!/usr/bin/env bash
# Installs the bundle for the current user, runs the command line program in
# the sandbox, and checks that the permissions are the ones the manifest
# asks for and no more (in particular no network and no file access).
#
#   smoke-test.sh <bundle.flatpak> <app-id>
set -euo pipefail

bundle=${1:?bundle}
id=${2:?app-id}

# The bundle names the Flathub runtime repository, which `install` fetches
# the runtime from; the remote must exist for that.
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y --noninteractive "$bundle"

flatpak run --command=fluut "$id" --version

perms=$(flatpak info --user --show-permissions "$id")
echo "$perms"
for bad in network filesystems= devices=all session-bus system-bus; do
  if echo "$perms" | grep -q "$bad"; then
    echo "unexpected permission: $bad" >&2
    exit 1
  fi
done
echo "$perms" | grep -q 'sockets=.*wayland' || { echo "wayland socket missing" >&2; exit 1; }
echo "$perms" | grep -q 'devices=dri' || { echo "dri device missing" >&2; exit 1; }

flatpak uninstall --user -y --noninteractive "$id"
if flatpak info --user "$id" >/dev/null 2>&1; then
  echo "$id is still installed after uninstall" >&2
  exit 1
fi
echo "smoke test passed"
