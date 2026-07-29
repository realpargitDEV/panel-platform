# Developer bootstrap. Idempotent: safe to re-run.

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

Write-Host 'Checking the toolchain...' -ForegroundColor Cyan

$missing = @()
foreach ($tool in @('node', 'pnpm', 'cargo', 'rustc', 'git')) {
    if ($null -eq (Get-Command $tool -ErrorAction SilentlyContinue)) {
        $missing += $tool
    } else {
        $version = (& $tool --version 2>&1 | Select-Object -First 1)
        Write-Host ("  {0,-8} {1}" -f $tool, $version)
    }
}

if ($missing.Count -gt 0) {
    Write-Host "`nMissing required tools: $($missing -join ', ')" -ForegroundColor Red
    Write-Host 'Install Node 22+, pnpm 11+, and a Rust stable toolchain, then re-run.'
    exit 1
}

# Optional, but the agent cannot run projects without it.
if ($null -eq (Get-Command docker -ErrorAction SilentlyContinue)) {
    Write-Host "`n  docker   NOT FOUND" -ForegroundColor Yellow
    Write-Host '  Everything builds and most tests run without it, but Docker'  -ForegroundColor Yellow
    Write-Host '  integration tests will be skipped and no project can start.' -ForegroundColor Yellow
} else {
    Write-Host ("  {0,-8} {1}" -f 'docker', (docker --version))
}

Write-Host "`nInstalling JavaScript dependencies..." -ForegroundColor Cyan
pnpm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`nGenerating API contracts from Rust..." -ForegroundColor Cyan
cargo run -q -p project-host-api-types --bin emit-contracts
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`nBuilding the workspace..." -ForegroundColor Cyan
cargo build --workspace
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`nReady. Next:" -ForegroundColor Green
Write-Host '  .\scripts\test-all.ps1    run every suite this host supports'
Write-Host '  pnpm contracts            regenerate TypeScript after changing Rust types'
