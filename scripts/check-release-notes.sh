#!/usr/bin/env bash
#
# The release notes become the `notes` field of `latest.json`, and every
# installed copy of the application refuses a manifest whose notes exceed
# `MAX_NOTES_LENGTH`. That limit is *compiled into the client*, so raising the
# constant does nothing for the copies already out there: a manifest that grows
# past what 0.1.13 accepts is a manifest 0.1.13 will never install, and the
# failure reaches the user as "the update check failed" with no way to act on
# it.
#
# The notes chain appends a section per release, so this grows on its own. At
# 0.1.14 it stood at 17,121 of 20,000 — one release from breaking updates for
# everyone. Hence a gate rather than a note in a document.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

workflow=".github/workflows/release.yml"
manifest="crates/updater/src/manifest.rs"

# The limit the clients actually enforce, read from the source of truth rather
# than repeated here where it could drift out of step with it.
limit="$(sed -n 's/^pub const MAX_NOTES_LENGTH: usize = \([0-9_]*\);.*/\1/p' "$manifest" |
  head -n 1 | tr -d '_')"
if [ -z "$limit" ]; then
  echo "error: could not read MAX_NOTES_LENGTH from $manifest" >&2
  exit 1
fi

# Keep a real margin. A release whose notes land just under the limit is one
# sentence away from the failure this exists to prevent.
margin=3000
budget=$((limit - margin))

# The `releaseBody: |` block scalar, dedented. The body is indented twelve
# spaces; the block ends at the next key, which is indented ten.
notes="$(awk '
  /^          releaseBody: \|/ { inside = 1; next }
  inside && /^          [^ ]/  { inside = 0 }
  inside                       { sub(/^            /, ""); print }
' "$workflow")"

if [ -z "$notes" ]; then
  echo "error: found no releaseBody block in $workflow" >&2
  exit 1
fi

length="${#notes}"

printf 'Release notes: %s characters\n' "$length"
printf '  client limit (MAX_NOTES_LENGTH)  %s\n' "$limit"
printf '  budget (limit less %s margin)  %s\n' "$margin" "$budget"

if [ "$length" -gt "$budget" ]; then
  echo >&2
  echo "error: the release notes are $length characters, over the $budget budget." >&2
  echo "Every installed client refuses a manifest whose notes exceed $limit, and" >&2
  echo "that limit is compiled into copies already released — raising it here" >&2
  echo "would not help them. Shorten the notes instead: drop the oldest" >&2
  echo "'What changed in ...' sections, which are on the releases page anyway." >&2
  exit 1
fi

echo "Within budget."
