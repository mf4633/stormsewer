<#
.SYNOPSIS
    Fails if a shipped binary imports the Visual C++ runtime.

.DESCRIPTION
    vcruntime140.dll ships with the Visual C++ Redistributable, not with
    Windows. rustc links it by default, so a stock Rust build dies with
    STATUS_DLL_NOT_FOUND (0xC0000135) before main() on any machine without the
    redist - bare VMs, imaged corporate boxes, and Microsoft's own winget
    validation sandbox, which is where it caught us (winget-pkgs#424771).

    .cargo/config.toml links the CRT statically instead. Nothing on a build
    machine can notice when that regresses: every CI runner and every dev box
    already has the redist installed, and the app runs fine. So this check
    reads the PE import table directly rather than trying to launch anything.

.EXAMPLE
    scripts/check-no-vcruntime.ps1 target/release/StormSewer.exe
#>
[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Path = @('target/release/StormSewer.exe')
)

$ErrorActionPreference = 'Stop'

function Get-PeImports {
    param([string]$File)

    $b = [System.IO.File]::ReadAllBytes($File)
    $pe = [BitConverter]::ToInt32($b, 0x3c)
    if ([BitConverter]::ToUInt32($b, $pe) -ne 0x00004550) { throw "$File is not a PE image" }

    $nSections = [BitConverter]::ToUInt16($b, $pe + 6)
    $optSize = [BitConverter]::ToUInt16($b, $pe + 20)
    $opt = $pe + 24
    $magic = [BitConverter]::ToUInt16($b, $opt)
    # Data directories follow the optional header's standard fields: 112 bytes
    # in for PE32+, 96 for PE32. Entry 1 is the import directory.
    $dirs = $opt + $(if ($magic -eq 0x20b) { 112 } else { 96 })
    $importRva = [BitConverter]::ToUInt32($b, $dirs + 8)
    if ($importRva -eq 0) { return @() }

    $sections = foreach ($i in 0..($nSections - 1)) {
        $s = $opt + $optSize + ($i * 40)
        [pscustomobject]@{
            Rva  = [BitConverter]::ToUInt32($b, $s + 12)
            Size = [Math]::Max([BitConverter]::ToUInt32($b, $s + 8), [BitConverter]::ToUInt32($b, $s + 16))
            Raw  = [BitConverter]::ToUInt32($b, $s + 20)
        }
    }
    function rvaToOffset([uint32]$rva) {
        foreach ($s in $sections) {
            if ($rva -ge $s.Rva -and $rva -lt $s.Rva + $s.Size) { return $s.Raw + ($rva - $s.Rva) }
        }
        throw "RVA 0x$($rva.ToString('x')) is outside every section"
    }

    $names = @()
    $off = rvaToOffset $importRva
    while ($true) {
        # Import descriptors are 20 bytes and the array ends with an all-zero one.
        $nameRva = [BitConverter]::ToUInt32($b, $off + 12)
        if ($nameRva -eq 0) { break }
        $p = rvaToOffset $nameRva
        $end = $p; while ($b[$end] -ne 0) { $end++ }
        $names += [Text.Encoding]::ASCII.GetString($b, $p, $end - $p)
        $off += 20
    }
    return $names
}

$bad = $false
foreach ($file in $Path) {
    if (-not (Test-Path $file)) { throw "no such file: $file" }
    $full = (Resolve-Path $file).Path
    $imports = Get-PeImports $full
    $redist = $imports | Where-Object { $_ -match '^(vcruntime|msvcp|msvcr|concrt)\d' }
    if ($redist) {
        Write-Host "FAIL $full imports $($redist -join ', ')" -ForegroundColor Red
        $bad = $true
    }
    else {
        Write-Host "ok   $full ($($imports.Count) imports, no VC++ runtime)"
    }
}

if ($bad) {
    Write-Error 'Shipped binary needs the Visual C++ Redistributable. Build with .cargo/config.toml in place (-C target-feature=+crt-static).'
    exit 1
}
