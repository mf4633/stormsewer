# StormSewer v0.9.3

Fixes three things that were invisible from a normal desktop: StormSewer would
not start at all on machines without an OpenGL driver, it said nothing when that
happened, and its in-app support links were dead.

## Fixed

- **Starts on remote desktop, Citrix/VDI, and virtual machines.** The app was
  built against OpenGL only. On a machine with no usable GL driver — RDP
  sessions, virtual desktops, plain VMs, and Microsoft's own winget validation
  sandbox — it simply failed to launch. Both renderers now ship, and startup
  tries Direct3D 12 (with a software adapter as a last resort) before falling
  back to OpenGL.
- **Says something when it cannot start.** A failed launch used to print to a
  console that a double-click does not have, so nothing appeared to happen at
  all. It now explains why, names the likely cause, and points at the browser
  build, which needs no graphics driver.
- **The support links work.** The Help menu item and the About dialog button
  pointed at a Buy Me a Coffee account that does not exist, so anyone who tried
  to say thanks hit a dead page. Both now open the real checkout.
- **The version is read from the build.** The window title, the document title,
  and the About dialog carried a hardcoded `v0.9` that had already gone stale
  once. All three now read the released version, with a test that fails on any
  new literal.

## Install

```sh
brew tap mf4633/tap
brew install --cask mf4633/tap/stormsewer   # macOS app
brew install mf4633/tap/stormsewer-cli      # macOS + Linux CLI
```

The engine is on crates.io (`cargo add stormsewer`) and compiles to WebAssembly.

| Platform | Download |
| --- | --- |
| Windows | `StormSewer-0.9.3-setup.exe` |
| macOS (Intel + Apple Silicon) | `StormSewer-macos-universal.zip` |
| Linux | `StormSewer-x86_64.AppImage` or `StormSewer-linux-x64.tar.gz` |
| Command line | `stormsewer-cli-linux-x64.tar.gz` / `stormsewer-cli-macos.tar.gz` |
| Browser build (engine only) | `stormsewer-web.zip` |

Windows and macOS builds are unsigned — SmartScreen and Gatekeeper will warn on
first run. On macOS, right-click the app and choose Open.

## Also since 0.9.1

- [**VALIDATION.md**](https://github.com/mf4633/stormsewer/blob/master/VALIDATION.md)
  works every number on a reference network by hand — intensity, Manning
  capacity, Rational accumulation, Tc accumulation, partial-flow velocity, HGL
  with junction loss, HEC-22 interception — and matches the engine to six
  decimal places, with a test that fails if any published number moves.
- **Python bindings.** `pip install stormsewer` gives the same engine as a
  native extension: primitives for scripting, and whole-network analysis
  returning dictionaries that drop straight into pandas.
- **Project files carry a format version**, with the promise that any 1.x
  StormSewer opens any 1.x project.
- Preferences and unsaved-work recovery now work on macOS and Linux (0.9.2).

293 tests.

## Support

Bugs and feature requests belong in
[Issues](https://github.com/mf4633/stormsewer/issues) — free, and the fastest
way to get something fixed. For commercial support, custom modules, or
firm-wide rollouts: support@hydrocomplete.com. If StormSewer saves you an
afternoon, [buy me a coffee](https://buy.stripe.com/14A3cudxo91z1qo0OHdAk00?client_reference_id=stormsewer-release).

Built by Michael Flynn, PE — see also [HydroComplete](https://hydrocomplete.com)
for browser-based hydrology and hydraulics.
