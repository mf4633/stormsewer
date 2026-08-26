# winget manifests

Manifests for the Windows Package Manager, so `winget install StormSewer` works.
One directory per released version; each holds the three files winget requires
(version, installer, default locale).

## Submitting a version

1. Build the release (tag push fires `.github/workflows/release.yml`) and wait
   for `StormSewer-<version>-setup.exe` to appear on the release.
2. Copy the previous version's directory, bump `PackageVersion` in all three
   files, and update `InstallerUrl`, `InstallerSha256` (uppercase),
   `ReleaseDate`, and `ReleaseNotesUrl`.

   ```sh
   gh release download v<version> -R mf4633/stormsewer -p "StormSewer-*-setup.exe"
   sha256sum StormSewer-<version>-setup.exe   # or: winget hash <file>
   ```

3. Validate locally — this must pass before submitting:

   ```powershell
   winget validate --manifest packaging\winget\<version>
   winget install --manifest packaging\winget\<version>   # optional smoke test
   ```

4. Submit to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs):
   fork it, copy the directory to
   `manifests/m/MichaelFlynn/StormSewer/<version>/`, and open a PR. The
   `wingetcreate` tool automates all of this:

   ```powershell
   wingetcreate update MichaelFlynn.StormSewer --version <version> `
     --urls https://github.com/mf4633/stormsewer/releases/download/v<version>/StormSewer-<version>-setup.exe `
     --submit
   ```

   Microsoft's pipeline then validates the manifest and installs the package in
   a sandbox; a maintainer merges it, usually within a day or two.

## Notes

- `Scope: user` — the installer sets `PrivilegesRequired=lowest`, so it installs
  per-user and needs no elevation.
- `ProductCode` is the Inno Setup `AppId` with the `_is1` suffix, which is how
  winget finds the package in the uninstall registry.
- `Publisher` must stay in step with `AppPublisher` in
  `installer/stormsewer.iss`.
