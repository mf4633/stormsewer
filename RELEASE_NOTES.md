# StormSewer v0.9.2

A correctness release for macOS and Linux, plus new ways to install.
GPL-3.0-or-later; free for the world.

## Fixed

- **Preferences and unsaved-work recovery now work on macOS and Linux.** The
  per-user data directory only ever consulted `%APPDATA%`, so on macOS and
  Linux it fell through to a relative path. A bundled app launched from Finder
  runs with `/` as its working directory, which meant preferences and the
  autosave recovery file were silently discarded on both platforms. StormSewer
  now uses `~/Library/Application Support/StormSewer` on macOS and
  `$XDG_CONFIG_HOME/stormsewer` (or `~/.config/stormsewer`) elsewhere; Windows
  behaviour is unchanged.
- **The macOS app reports its real version.** The bundle carried a hardcoded
  `0.7.0`, which Finder displayed and Homebrew's upgrade check believed. The
  release build now stamps it from the tag.

Windows is unaffected by both fixes — 0.9.1 and 0.9.2 are functionally
identical there.

## Install

`winget` and Homebrew now work alongside the direct downloads:

```sh
brew tap mf4633/tap
brew install --cask mf4633/tap/stormsewer   # macOS app
brew install mf4633/tap/stormsewer-cli      # macOS + Linux CLI
```

The engine is also on crates.io — `cargo add stormsewer` — and compiles to
WebAssembly.

| Platform | Download |
| --- | --- |
| Windows | `StormSewer-0.9.2-setup.exe` |
| macOS (Intel + Apple Silicon) | `StormSewer-macos-universal.zip` |
| Linux | `StormSewer-x86_64.AppImage` or `StormSewer-linux-x64.tar.gz` |
| Command line | `stormsewer-cli-linux-x64.tar.gz` / `stormsewer-cli-macos.tar.gz` |
| Browser build | `stormsewer-web.zip` |

Windows and macOS builds are unsigned — SmartScreen and Gatekeeper will warn on
first run. On macOS, right-click the app and choose Open.

## From 0.9.1

If you missed it, 0.9.1 rewrote the PDF report into a submittal document —
title block on every page, ruled pipe / structure / HEC-22 inlet schedules, a
scaled plan, and a profile with real elevation and station axes — and put it
behind a Report Options dialog where you choose the sections, fill the title
block, preview, and pick where it saves.

## Support

Bugs and feature requests belong in
[Issues](https://github.com/mf4633/stormsewer/issues) — free, and the fastest
way to get something fixed. For commercial support, custom modules, or
firm-wide rollouts: support@hydrocomplete.com. If StormSewer saves you an
afternoon, [buy me a coffee](https://buy.stripe.com/14A3cudxo91z1qo0OHdAk00?client_reference_id=stormsewer-release).

Built by Michael Flynn, PE — see also [HydroComplete](https://hydrocomplete.com)
for browser-based hydrology and hydraulics.
