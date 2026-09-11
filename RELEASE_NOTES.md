# StormSewer v0.9.8

**If StormSewer has never started on one of your machines — a virtual desktop, a
freshly imaged laptop, a build server — install this release.** It starts there
now. Nothing about the hydraulics changed; every number in v0.9.7 is unchanged.

## Fixed

- **The app needed the Visual C++ Redistributable and never said so.** On any
  Windows machine without it, double-clicking StormSewer did nothing at all:
  Windows failed to load the program before a single line of it ran
  (`STATUS_DLL_NOT_FOUND`, `0xC0000135`), with no window, no error, and nothing
  in the log. Rust links `vcruntime140.dll` by default, and that DLL ships with
  Microsoft's redistributable, not with Windows.

  This release links the C runtime into the executable instead, so there is no
  redistributable to install and nothing to go missing. It costs 230 KB. The
  fix reaches every download: the installer, the portable zip, Chocolatey, and
  Scoop.

  Microsoft's winget validation sandbox found it — a clean Windows image is
  exactly the case nobody tests on. The bundled Mesa software-OpenGL fallback
  from v0.9.5 was failing for the same reason, so machines with no GPU driver
  had no path at all.

  `scripts\check-no-vcruntime.ps1` now reads the executable's import table on
  every release and smoke build and fails if a runtime dependency comes back.
  A launch test cannot catch this: every build machine already has the
  redistributable installed, which is how it shipped in the first place.

## Unchanged

- Manning's K stays 1.486 and every capacity matches v0.9.7 exactly. This is a
  packaging fix; sealed sheets produced with v0.9.7 reproduce here.
