#!/usr/bin/env bash
#
# Fail if any tracked file starts with a UTF-8 byte-order mark.
#
# Cargo, serde and rustc all tolerate a BOM, so one can sit in a manifest for
# months without any local command complaining. The TOML parser inside
# `tauri-action` does not tolerate it: it stops with
#
#     Unknown character "65279" at row 1, col 2, pos 1
#
# which cost a full release build to diagnose. Windows editors and
# `Out-File`/`Set-Content` add BOMs by default, so this is easy to reintroduce
# by hand and worth a cheap check.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

found=0
while IFS= read -r file; do
  [ -f "$file" ] || continue
  if [ "$(head -c 3 "$file" | od -An -tx1 | tr -d ' \n')" = "efbbbf" ]; then
    echo "error: $file starts with a UTF-8 BOM" >&2
    found=1
  fi
done < <(git ls-files)

if [ "$found" -ne 0 ]; then
  echo >&2
  echo "Strip it, for example:" >&2
  echo "  perl -i -pe 's/^\\x{ef}\\x{bb}\\x{bf}// if \$. == 1' <file>" >&2
  exit 1
fi

echo "No tracked file starts with a BOM."
