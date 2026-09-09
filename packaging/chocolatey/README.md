# Chocolatey package

Ready to build and push. It needs a chocolatey.org account and an API key,
which is why it stops here rather than being on the community feed already.

```powershell
cd packaging\chocolatey
choco pack
choco apikey --key <your-key> --source https://push.chocolatey.org/
choco push stormsewer.0.9.6.nupkg --source https://push.chocolatey.org/
```

Then test the built package locally before pushing, from an elevated prompt:

```powershell
choco install stormsewer --source . --force
choco uninstall stormsewer
```

## What a moderator will look at

- The installer is **downloaded** from the GitHub release rather than embedded,
  so `tools/VERIFICATION.txt` carries the URL, the SHA256, and how to check it.
- `silentArgs` are the Inno Setup ones. `/SP-` suppresses the "This will
  install..." prompt, which otherwise hangs an unattended install.
- Uninstall is left to Chocolatey's auto-uninstaller. That works because the
  installer registers an Add/Remove Programs entry whose DisplayName is exactly
  `StormSewer`, which `softwareName` matches.
- The package and the software have the same author, so there is no third-party
  redistribution question.

## On every release

Three things move: `<version>` in the nuspec, and the URL and `checksum64` in
`tools/chocolateyinstall.ps1`. Get the checksum from the published asset, not
from a local build, or the two will differ:

```powershell
(Get-FileHash .\StormSewer-<version>-setup.exe -Algorithm SHA256).Hash.ToLower()
```

`tools/VERIFICATION.txt` repeats both and has to be updated with them.
