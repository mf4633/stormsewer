# StormSewer headless test suite — engine integration, CLI subprocess, app logic (no GUI).
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

Write-Host "=== StormSewer headless tests ===" -ForegroundColor Cyan

Write-Host "`n[1/3] Engine unit tests..." -ForegroundColor Yellow
cargo test -p stormsewer --lib --quiet
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`n[2/3] Headless integration + CLI tests..." -ForegroundColor Yellow
cargo test -p stormsewer --test headless_suite --test cli_headless --quiet
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`n[3/3] App logic tests (edit/prefs, no window)..." -ForegroundColor Yellow
cargo test -p stormsewer-app --quiet
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`nAll headless tests passed." -ForegroundColor Green