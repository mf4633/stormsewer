<#
.SYNOPSIS
    Builds the 32-bit SWMM engine bridge into target\.

.DESCRIPTION
    EPA ships swmm5.dll as a 32-bit DLL and StormSewer is 64-bit, so the
    step-by-step engine control (live 1D-2D coupling, real-time control,
    docs\23-live-runs.md) runs through a small helper process,
    stormsewer-swmm-bridge32.exe, built for i686-pc-windows-msvc from the
    bridge\ crate. This script adds the target if it is missing, builds the
    helper in release, checks it does not need the Visual C++ Redistributable
    (.cargo\config.toml links the CRT statically for i686 too), and prints
    where it landed. The app finds it beside StormSewer.exe, through
    STORMSEWER_SWMM_BRIDGE, or - in a debug build - in this target folder.

.PARAMETER Copy
    Also copy the helper beside target\release\StormSewer.exe (and the mesa\
    copy) so a locally built app picks it up.

.EXAMPLE
    scripts\build-bridge.ps1
    scripts\build-bridge.ps1 -Copy
#>
[CmdletBinding()]
param(
    [switch]$Copy
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $target = 'i686-pc-windows-msvc'
    $installed = & rustup target list --installed
    if ($installed -notcontains $target) {
        Write-Host "Adding the $target target..."
        & rustup target add $target
        if ($LASTEXITCODE -ne 0) { throw "rustup target add $target failed" }
    }

    & cargo build -p stormsewer-swmm-bridge --release --target $target
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

    $exe = Join-Path $root "target\$target\release\stormsewer-swmm-bridge32.exe"
    if (-not (Test-Path $exe)) { throw "expected $exe after the build" }
    & (Join-Path $PSScriptRoot 'check-no-vcruntime.ps1') $exe
    if ($LASTEXITCODE -ne 0) { throw 'the bridge imports the VC++ runtime' }

    $version = & $exe --version
    Write-Host "built $version -> $exe"

    if ($Copy) {
        foreach ($dir in @('target\release', 'target\release\mesa')) {
            $dest = Join-Path $root $dir
            if (Test-Path $dest) {
                Copy-Item $exe $dest -Force
                Write-Host "copied to $dest"
            }
        }
    }
}
finally {
    Pop-Location
}
