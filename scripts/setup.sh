#!/usr/bin/env bash
# Developer bootstrap. Idempotent: safe to re-run.

set -euo pipefail
cd "$(dirname "$0")/.."

echo 'Checking the toolchain...'

missing=()
for tool in node pnpm cargo rustc git; do
  if command -v "$tool" >/dev/null 2>&1; then
    printf '  %-8s %s\n' "$tool" "$("$tool" --version 2>&1 | head -n1)"
  else
    missing+=("$tool")
  fi
done

if [ ${#missing[@]} -gt 0 ]; then
  printf '\nMissing required tools: %s\n' "${missing[*]}"
  echo 'Install Node 22+, pnpm 11+, and a Rust stable toolchain, then re-run.'
  exit 1
fi

# Optional, but the agent cannot run projects without it.
if command -v docker >/dev/null 2>&1; then
  printf '  %-8s %s\n' docker "$(docker --version)"
else
  printf '\n  docker   NOT FOUND\n'
  echo '  Everything builds and most tests run without it, but Docker'
  echo '  integration tests will be skipped and no project can start.'
fi

echo
echo 'Installing JavaScript dependencies...'
pnpm install

echo
echo 'Generating API contracts from Rust...'
cargo run -q -p project-host-api-types --bin emit-contracts

echo
echo 'Building the workspace...'
cargo build --workspace

echo
echo 'Ready. Next:'
echo '  ./scripts/test-all.sh     run every suite this host supports'
echo '  pnpm contracts            regenerate TypeScript after changing Rust types'
