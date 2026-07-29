# Runs every suite that can run on this machine, and reports what it skipped.
#
# The skip list is the point. This project targets two operating systems and
# depends on Docker; a summary that quietly omitted the suites it could not run
# would be worse than no summary. See docs/testing-strategy.md §2.

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$failures = @()
$skipped = @()

function Invoke-Step {
    param([string]$Name, [scriptblock]$Body)
    Write-Host "`n--- $Name" -ForegroundColor Cyan
    try {
        & $Body
        if ($LASTEXITCODE -ne 0) { throw "$Name exited with $LASTEXITCODE" }
        Write-Host "    ok" -ForegroundColor Green
    } catch {
        Write-Host "    FAILED: $_" -ForegroundColor Red
        $script:failures += $Name
    }
}

# ---- what this host can do
$hasDocker = $null -ne (Get-Command docker -ErrorAction SilentlyContinue)
if (-not $hasDocker) {
    $skipped += 'Docker integration tests (no docker command on PATH)'
}
$skipped += 'Linux platform suite (systemd, .deb, UFW) — requires an Ubuntu/Debian host'
$skipped += 'Installer tests — require virtual machines'
$skipped += 'Remote pairing across machines — requires a second host'

Invoke-Step 'contract is up to date' { cargo run -q -p project-host-api-types --bin emit-contracts -- --check }
Invoke-Step 'cargo fmt'              { cargo fmt --all -- --check }
Invoke-Step 'cargo clippy'           { cargo clippy --workspace --all-targets -- -D warnings }
Invoke-Step 'cargo test'             { cargo test --workspace }
Invoke-Step 'tsc'                    { pnpm typecheck }
Invoke-Step 'eslint'                 { pnpm lint }
Invoke-Step 'prettier'               { pnpm format:check }
Invoke-Step 'vitest'                 { pnpm test }

if ($hasDocker) {
    Invoke-Step 'docker integration' { cargo test --workspace --features docker-tests }
}

Write-Host "`n==================== summary ====================" -ForegroundColor Cyan
if ($skipped.Count -gt 0) {
    Write-Host "`nSKIPPED (not verified, not passing):" -ForegroundColor Yellow
    $skipped | ForEach-Object { Write-Host "  - $_" -ForegroundColor Yellow }
}
if ($failures.Count -gt 0) {
    Write-Host "`nFAILED:" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "`nAll suites that can run on this host passed." -ForegroundColor Green
exit 0
