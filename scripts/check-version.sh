#!/usr/bin/env bash
#
# The version of a release lives in four places, and a release where they
# disagree is worse than one that never built: the installer filename, the
# window's About box and the updater's comparison would each report something
# different. This fails the release before that can happen.
#
#   scripts/check-version.sh v0.1.0     compare a tag against the tree
#   scripts/check-version.sh            check only that the tree agrees
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

read_json_version() {
  # Deliberately not `jq` — this runs before dependencies are installed, and on
  # a runner where jq's presence is not guaranteed.
  sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -n 1
}

tauri_version="$(read_json_version apps/desktop/src-tauri/tauri.conf.json)"
package_version="$(read_json_version apps/desktop/package.json)"
root_package_version="$(read_json_version package.json)"
cargo_version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' Cargo.toml | head -n 1)"

failed=0
report() {
  printf '  %-34s %s\n' "$1" "$2"
}

echo "Versions found:"
report "apps/desktop/src-tauri/tauri.conf.json" "$tauri_version"
report "apps/desktop/package.json" "$package_version"
report "package.json" "$root_package_version"
report "Cargo.toml [workspace.package]" "$cargo_version"
echo

for pair in \
  "apps/desktop/package.json:$package_version" \
  "package.json:$root_package_version" \
  "Cargo.toml:$cargo_version"; do
  file="${pair%%:*}"
  value="${pair#*:}"
  if [ "$value" != "$tauri_version" ]; then
    echo "error: $file says '$value' but tauri.conf.json says '$tauri_version'" >&2
    failed=1
  fi
done

if [ "$#" -ge 1 ] && [ -n "${1:-}" ]; then
  tag="$1"
  # Tags are written `v0.1.0`; the manifests carry the bare version.
  tag_version="${tag#v}"
  if [ "$tag_version" != "$tauri_version" ]; then
    echo "error: tag '$tag' does not match the version in the tree ('$tauri_version')" >&2
    echo "hint: bump the four files above, or tag v$tauri_version instead" >&2
    failed=1
  else
    echo "Tag '$tag' matches the tree."
  fi
fi

if [ "$failed" -ne 0 ]; then
  exit 1
fi

echo "All versions agree on $tauri_version."
