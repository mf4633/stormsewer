# Build the Microsoft Store package (.msix) for StormSewer.
#
#   scripts/build-msix.ps1 -IdentityName <from Partner Center> -Publisher "CN=..." [-Version 0.9.5.0] [-Exe target/release/StormSewer.exe]
#
# Partner Center > Product identity gives IdentityName and Publisher after the
# app name is reserved; the Store re-signs the package, so no local signing
# is needed for a Store submission. Sideload testing of an unsigned package
# needs Developer Mode plus a self-signed cert (see -SelfSign).
#
# Layout packed: StormSewer.exe, examples\, mesa\ (software OpenGL fallback,
# staged by scripts/fetch-mesa.ps1), Assets\, AppxManifest.xml.
param(
    [Parameter(Mandatory)] [string]$IdentityName,
    [Parameter(Mandatory)] [string]$Publisher,
    [string]$Version = "",
    [string]$Exe = "target/release/StormSewer.exe",
    [string]$Out = "dist/StormSewer.msix",
    [switch]$SelfSign
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
if (-not $Version) {
    $v = (Select-String -Path "$root/app/Cargo.toml" -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
    $Version = "$v.0"
}
$kit = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" | Sort-Object FullName | Select-Object -Last 1
if (-not $kit) { throw "makeappx.exe not found (install the Windows 10/11 SDK)" }
$makeappx = $kit.FullName
$signtool = Join-Path (Split-Path $makeappx) "signtool.exe"

$stage = Join-Path $root "target/msix-stage"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force "$stage/Assets", "$stage/examples", "$stage/mesa" | Out-Null

$exePath = Join-Path $root $Exe
if (-not (Test-Path $exePath)) { throw "$Exe not built" }
Copy-Item $exePath $stage
Copy-Item "$root/examples/demo.ssproj", "$root/examples/investor-demo.ssproj" "$stage/examples"
$mesa = Join-Path (Split-Path $exePath) "mesa"
if (Test-Path "$mesa/libgallium_wgl.dll") {
    Copy-Item "$mesa/*" "$stage/mesa"
} else {
    Write-Warning "no mesa\ beside $Exe (run scripts/fetch-mesa.ps1); package will have no software-GL fallback"
    Remove-Item "$stage/mesa"
}
Copy-Item "$root/packaging/msix/Assets/*" "$stage/Assets"
(Get-Content "$root/packaging/msix/AppxManifest.xml" -Raw) `
    -replace '__IDENTITY_NAME__', $IdentityName `
    -replace '__PUBLISHER__', $Publisher `
    -replace '__VERSION__', $Version | Set-Content "$stage/AppxManifest.xml" -Encoding UTF8

New-Item -ItemType Directory -Force (Split-Path (Join-Path $root $Out)) | Out-Null
$outPath = Join-Path $root $Out
& $makeappx pack /d $stage /p $outPath /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }

if ($SelfSign) {
    $cert = New-SelfSignedCertificate -Type Custom -Subject $Publisher -KeyUsage DigitalSignature -FriendlyName "StormSewer MSIX test" -CertStoreLocation "Cert:\CurrentUser\My" -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
    & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $outPath | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "signtool failed ($LASTEXITCODE)" }
    Write-Host "Self-signed with $($cert.Thumbprint); import it to Trusted People to sideload."
}
$size = (Get-Item $outPath).Length
Write-Host ("Built {0} ({1:N1} MB), identity {2}, version {3}" -f $Out, ($size/1MB), $IdentityName, $Version)
