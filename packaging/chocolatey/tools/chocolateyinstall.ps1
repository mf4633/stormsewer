$ErrorActionPreference = 'Stop'

# The installer is not embedded in the package. Chocolatey's moderators prefer
# a download from the vendor's own release, and it keeps the package small.
# The checksum below is the published asset's, so a swapped file fails loudly.
$packageArgs = @{
    packageName    = 'stormsewer'
    fileType       = 'exe'
    url64bit       = 'https://github.com/mf4633/stormsewer/releases/download/v0.9.7/StormSewer-0.9.7-setup.exe'
    checksum64     = '4a878a427c41f7590b805415aa1282d5230f492d22a56aca1044f12c6de6abc0'
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
