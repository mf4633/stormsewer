# Fetch the pinned Mesa llvmpipe build that the Windows installer bundles as
# the software-OpenGL fallback (see app/src/software_gl.rs and ROADMAP.md),
# and stage it as the `mesa` folder beside a built StormSewer.exe.
#
#   scripts/fetch-mesa.ps1 [-Exe target/release/StormSewer.exe] [-Out target/release/mesa]
#
# Produces <Out>\opengl32.dll, <Out>\libgallium_wgl.dll, <Out>\NOTICE.txt and
# <Out>\StormSewer.exe (a copy of -Exe: the fallback must run from the folder
# that holds Mesa's opengl32.dll, because the executable imports it statically).
#
# Source: https://github.com/pal1000/mesa-dist-win (MSVC release build). The
# archive is verified against a pinned SHA-256 before anything is extracted.
param(
    [string]$Exe = "target/release/StormSewer.exe",
    [string]$Out = "target/release/mesa",
    [string]$Version = "26.2.0",
    [string]$Sha256 = "dcb2719ef346dab5b609fcb193a5f13cfc4b0502e3f4de1ad43d349477402f47"
)
$ErrorActionPreference = "Stop"

$archive = "mesa3d-$Version-release-msvc.7z"
$url = "https://github.com/pal1000/mesa-dist-win/releases/download/$Version/$archive"
$cache = Join-Path ([IO.Path]::GetTempPath()) "stormsewer-mesa"
New-Item -ItemType Directory -Force $cache | Out-Null
$path = Join-Path $cache $archive

if (-not (Test-Path $path) -or (Get-FileHash $path -Algorithm SHA256).Hash -ne $Sha256.ToUpper()) {
    Write-Host "Downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $path -UseBasicParsing
}
$hash = (Get-FileHash $path -Algorithm SHA256).Hash
if ($hash -ne $Sha256.ToUpper()) {
    throw "SHA-256 mismatch for ${archive}: got $hash, expected $Sha256"
}

# 7-Zip: the CI runner has 7z on PATH; otherwise use the standalone 7zr.
$sevenZip = Get-Command 7z -ErrorAction SilentlyContinue
if ($null -eq $sevenZip) {
    $sevenZipExe = Join-Path $cache "7zr.exe"
    if (-not (Test-Path $sevenZipExe)) {
        Invoke-WebRequest -Uri "https://www.7-zip.org/a/7zr.exe" -OutFile $sevenZipExe -UseBasicParsing
    }
} else {
    $sevenZipExe = $sevenZip.Source
}

$extract = Join-Path $cache "x-$Version"
New-Item -ItemType Directory -Force $extract | Out-Null
& $sevenZipExe x -y "-o$extract" $path "x64/opengl32.dll" "x64/libgallium_wgl.dll" | Out-Null
if ($LASTEXITCODE -ne 0) { throw "7-Zip extraction failed ($LASTEXITCODE)" }

New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item (Join-Path $extract "x64/opengl32.dll") $Out -Force
Copy-Item (Join-Path $extract "x64/libgallium_wgl.dll") $Out -Force
Copy-Item (Join-Path $PSScriptRoot "../packaging/mesa/NOTICE.txt") $Out -Force
if (Test-Path $Exe) {
    Copy-Item $Exe $Out -Force
} else {
    Write-Warning "$Exe not built yet; the fallback copy of the executable was not staged"
}
Get-ChildItem $Out | ForEach-Object { Write-Host ("  {0,-22} {1,10:N0} bytes" -f $_.Name, $_.Length) }
