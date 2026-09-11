$ErrorActionPreference = 'Stop'

# The installer is not embedded in the package. Chocolatey's moderators prefer
# a download from the vendor's own release, and it keeps the package small.
# The checksum below is the published asset's, so a swapped file fails loudly.
$packageArgs = @{
    packageName    = 'stormsewer'
    fileType       = 'exe'
    url64bit       = 'https://github.com/mf4633/stormsewer/releases/download/v0.9.8/StormSewer-0.9.8-setup.exe'
    checksum64     = 'c9e444aec7c28f1698a36c0de78dd11a3619f4cbfcfc513709762914a3b78f4d'
    checksumType64 = 'sha256'

    # Inno Setup. /SP- suppresses the "This will install..." prompt.
    silentArgs     = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-'
    validExitCodes = @(0, 3010, 1641)

    softwareName   = 'StormSewer*'
}

Install-ChocolateyPackage @packageArgs

Write-Host ''
Write-Host 'StormSewer is not code-signed, so SmartScreen may warn on first run.'
Write-Host 'Sample projects are in the examples folder of the install directory.'
