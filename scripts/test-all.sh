#!/usr/bin/env bash
# Runs every suite that can run on this machine, and reports what it skipped.
#
# The skip list is the point. This project targets two operating systems and
# depends on Docker; a summary that quietly omitted the suites it could not run
# would be worse than no summary. See docs/testing-strategy.md §2.

set -uo pipefail
cd "$(dirname "$0")/.."

failures=()
skipped=()

step() {
  local name="$1"
  shift
  printf '\n--- %s\n' "$name"
  if "$@"; then
    printf '    ok\n'
  else
    printf '    FAILED\n'
    failures+=("$name")
  fi
}

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  has_docker=1
else
  has_docker=0
  skipped+=('Docker integration tests (no reachable Docker daemon)')
fi

if [ "$(uname -s)" != "Linux" ]; then
  skipped+=('Linux platform suite (systemd, .deb, UFW) — requires an Ubuntu/Debian host')
fi

skipped+=('Installer tests — require virtual machines')
skipped+=('Remote pairing across machines — requires a second host')

step 'contract is up to date' cargo run -q -p project-host-api-types --bin emit-contracts -- --check
step 'cargo fmt'              cargo fmt --all -- --check
step 'cargo clippy'           cargo clippy --workspace --all-targets -- -D warnings
step 'cargo test'             cargo test --workspace
step 'tsc'                    pnpm typecheck
step 'eslint'                 pnpm lint
step 'prettier'               pnpm format:check
step 'vitest'                 pnpm test

if [ "$has_docker" -eq 1 ]; then
  step 'docker integration' cargo test --workspace --features docker-tests
fi

printf '\n==================== summary ====================\n'
if [ ${#skipped[@]} -gt 0 ]; then
  printf '\nSKIPPED (not verified, not passing):\n'
  for item in "${skipped[@]}"; do printf '  - %s\n' "$item"; done
fi
if [ ${#failures[@]} -gt 0 ]; then
  printf '\nFAILED:\n'
  for item in "${failures[@]}"; do printf '  - %s\n' "$item"; done
  exit 1
fi
printf '\nAll suites that can run on this host passed.\n'
