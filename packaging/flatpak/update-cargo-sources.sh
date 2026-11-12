#!/usr/bin/env bash
# Regenerates cargo-sources.json (the vendored crates of the Flathub manifest)
# from the workspace's Cargo.lock, or checks that it is up to date.
#
#   update-cargo-sources.sh            rewrite cargo-sources.json
#   update-cargo-sources.sh --check    fail if cargo-sources.json differs
#
# The generator is flatpak-cargo-generator.py from flatpak/flatpak-builder-tools,
# pinned to a commit and to the SHA-256 of that one file. It is downloaded into a
# scratch folder (never into the repository) and run with `python -I` inside a
# virtualenv. The crates come from Cargo.lock's own checksums, so the output
# depends on Cargo.lock and the generator only.
#
# Needs python3 (with venv), curl, sha256sum and network access to github.com
# and pypi.org. Set FLUUT_GEN_DIR to keep the scratch folder between runs.
set -euo pipefail

# Pinned generator: commit of flatpak/flatpak-builder-tools, and the SHA-256 of
# cargo/flatpak-cargo-generator.py at that commit. Change both together, and
# FLATHUB.md says which commit is in use.
GENERATOR_COMMIT=41c20aa10819cdb2a4f3ca171758a96d1955c018
GENERATOR_SHA256=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
target="$here/cargo-sources.json"

mode=write
case "${1:-}" in
  "") ;;
  --check) mode=check ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

work=${FLUUT_GEN_DIR:-$(mktemp -d)}
[ -n "${FLUUT_GEN_DIR:-}" ] || trap 'rm -rf "$work"' EXIT
mkdir -p "$work/gen"

gen="$work/gen/flatpak-cargo-generator.py"
if ! { [ -f "$gen" ] && echo "$GENERATOR_SHA256  $gen" | sha256sum -c --status -; }; then
  curl -sSfL -o "$gen" \
    "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$GENERATOR_COMMIT/cargo/flatpak-cargo-generator.py"
  echo "$GENERATOR_SHA256  $gen" | sha256sum -c - \
    || { echo "flatpak-cargo-generator.py does not match the pinned SHA-256" >&2; exit 1; }
fi

if [ ! -d "$work/venv" ]; then
  python3 -m venv "$work/venv" 2>/dev/null || python -m venv "$work/venv"
  py="$work/venv/bin/python"; [ -x "$py" ] || py="$work/venv/Scripts/python"
  "$py" -m pip install -q "aiohttp>=3.9.5,<4" "PyYAML>=6.0.2,<7" "tomlkit>=0.13.3,<1.0"
fi
py="$work/venv/bin/python"; [ -x "$py" ] || py="$work/venv/Scripts/python"

out="$work/cargo-sources.json"
"$py" -I "$gen" "$root/Cargo.lock" -o "$out"
# Windows writes CRLF; the repository stores LF.
# It writes no final newline either; the repository file has one.
{ tr -d '\r' < "$out"; echo; } > "$out.lf"

if [ "$mode" = check ]; then
  if ! diff -u "$target" "$out.lf" > "$work/diff.txt"; then
    head -n 40 "$work/diff.txt" >&2
    echo "cargo-sources.json is out of date with Cargo.lock; run packaging/flatpak/update-cargo-sources.sh and commit the result" >&2
    exit 1
  fi
  echo "cargo-sources.json is up to date"
else
  cp "$out.lf" "$target"
  echo "wrote $target"
fi
