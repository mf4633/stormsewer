# Build StormSewer release binary and Windows installer (Inno Setup 6).
# Usage: .\scripts\build-installer.ps1 [-Sign]

param(
    [switch]$Sign
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

Write-Host "Building StormSewer release..."
cargo build --release -p stormsewer-app
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path $Root "target\release\StormSewer.exe"
if (-not (Test-Path $exe)) {
    Write-Error "Release binary not found: $exe"
}

if ($Sign) {
    $signtool = Get-Command signtool -ErrorAction SilentlyContinue
    if (-not $signtool) {
        Write-Warning "signtool not found - skipping code signing. Install Windows SDK Signing Tools."
    } else {
        Write-Host "Signing $exe ..."
        & signtool sign /fd SHA256 /a /tr http://timestamp.digicert.com /td SHA256 $exe
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
}

$iscc = Get-Command iscc -ErrorAction SilentlyContinue
if (-not $iscc) {
    $candidate = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    if (Test-Path $candidate) { $iscc = $candidate }
}

if (-not $iscc) {
    Write-Warning "Inno Setup (iscc) not found. Binary built at: $exe"
    Write-Warning "Install Inno Setup 6 from https://jrsoftware.org/isinfo.php then run:"
    Write-Warning "  iscc installer\stormsewer.iss"
    exit 0
}

Write-Host "Building installer..."
& $iscc (Join-Path $Root "installer\stormsewer.iss")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Done. Installer: $Root\dist\StormSewer-0.5.0-setup.exe"