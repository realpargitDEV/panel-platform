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

# Cargo.lock pins the workspace's own crates alongside their dependencies, and
# bumping `[workspace.package]` does not rewrite it — the next `cargo` command
# does, quietly. So a release can be tagged from a tree whose lock file still
# names the previous version, and nothing above would notice: `cargo build` has
# no `--locked` here, so CI regenerates it and goes green while the committed
# lock file describes a build nobody made. 0.1.13 shipped exactly that way.
#
# Each member's package name comes from its own manifest rather than a prefix
# guess, so a crate renamed or added is covered without touching this script.
stale=""
while read -r member; do
  [ -n "$member" ] || continue
  manifest="$member/Cargo.toml"
  [ -f "$manifest" ] || continue
  name="$(sed -n 's/^name[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
  [ -n "$name" ] || continue
  # The version recorded for this package in the lock file: find its block by
  # name, then take the first `version` line after it.
  locked="$(
    awk -v want="$name" '
      $0 == "name = \"" want "\"" { found = 1; next }
      found && /^version = / {
        gsub(/^version = "|"$/, "")
        print
        exit
      }
    ' Cargo.lock
  )"
  [ -n "$locked" ] || continue
  if [ "$locked" != "$tauri_version" ]; then
    stale="$stale  $name $locked"$'\n'
    failed=1
  fi
done <<EOF
$(sed -n '/^members[[:space:]]*=[[:space:]]*\[/,/^]/p' Cargo.toml |
  sed -n 's/^[[:space:]]*"\([^"]*\)".*/\1/p')
EOF

if [ -n "$stale" ]; then
  echo "error: Cargo.lock still pins workspace crates at an old version:" >&2
  printf '%s' "$stale" >&2
  echo "hint: run 'cargo check --workspace' and commit the updated Cargo.lock" >&2
else
  report "Cargo.lock (workspace crates)" "$tauri_version"
fi

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
