# 23. Stopping a run and watching it live

A run is EPA's `runswmm` working on a scratch or saved copy of the model
([§9.2](09-run-and-engines.md)). This chapter covers what happens while it
works: stopping it, watching the results as they are written, and the
engine bridge that drives EPA's DLL one routing step at a time for the
uses that need more than a file at the end.

## 23.1 Stop

**Stop** — Run → Stop, the **Stop** button in the SWMM panel beside the
spinner, or **Stop** in the Live Results window — kills the engine
process. It does not ask the engine to finish politely: `runswmm` has no
way to be told, so the process is ended and StormSewer keeps whatever it had
written. The status line reads `Stopping…` for the few tens of milliseconds
the worker thread takes to notice, then
`SWMM: stopped by user after N s`.

What is left on disk:

- The `.rpt` holds the input summary and any warnings the engine had
  flushed before it died. It has no continuity section and no summary
  tables. The SWMM panel lists it under **Stopped run** with the errors and
  warnings it does contain; Run Status does not open, because a partial
  report is not a finished run and must not enter the run history
  ([§22](22-scenarios-calibration.md)) or the report builder.
- The `.out` has its header and one record per reporting period the engine
  reached, but not the closing block a finished file ends with. StormSewer
  opens it as a *partial* results file (§23.2): the map, chart, results
  view, profile and tables all work over the periods that exist, and the
  panel's results heading says **Results (partial)**.
- `[FILES] SAVE` outputs (hotstart, RDII, routing interface files) are
  whatever the engine had written; they are not copied back from a scratch
  folder.
- A stopped run does not become the last run: **Run Status…**, **Run
  Report…** and the run history keep the previous finished run.

Running the model again clears the stopped run and starts clean, deleting
the partial `.rpt` and `.out` first as every run does.

The 2D and coupled runs have their own **Stop 2D**
([§20.4](20-2d-overland.md)); Stop here is the 1D engine only.

## 23.2 Live results

While the engine runs, it writes the binary `.out` one reporting period at
a time. Run → **Live Results** (on by default) makes StormSewer read that
file every half second and open a **Live Results** window when a run
starts:

| Line | What it is |
|---|---|
| progress bar | Simulation time reached against the model's duration (`START_DATE/TIME` to `END_DATE/TIME` from `[OPTIONS]`) |
| **Simulation time** | Date and time of the newest reporting period |
| **Periods written** | Whole records on disk, and the report step |
| **Flooding now** | Nodes whose overflow is above zero in the newest period (red when any are) |
| **Deepest node** | The node with the greatest depth in the newest period |
| **Fullest link** | The link nearest to (or over) full in the newest period |
| **Wall clock** | Seconds since the engine was launched |

**Follow** (on by default) moves the map's period slider to the newest
period each time one arrives, so the network animates as the engine works
— nodes turn red as they flood, links fill, catchments light up. Turn it
off to hold the map on one period while the run continues; the period
slider's range still grows. **Stop** is the same Stop as above. Closing
the window does not stop the run or the reading; it stays closed until the
next run. When the run ends the window closes and the panel loads the
finished results as usual.

**Partial `.out`.** A finished results file ends with a closing block —
where the records start, how many there are, the engine's error code and a
magic number — and the normal reader trusts it. A file still being written
has no closing block, so the live reader walks the header forward instead
(magic, counts, object names, input properties, variable selections, start
date, report step; the layout EPA's `output.c` writes) and counts whole
records from the file size. Everything the results views read — series,
peaks, frames — then works over that count. Re-reading picks up new
records; the count never goes down.

Caveats:

- The engine writes through the C runtime's buffered I/O, so records reach
  the file in 4 KB bursts. On a small model that is several periods at
  once, and the count lags the engine by up to a burst. It catches up at
  the end of the run.
- Peaks shown during a run (the map's *run peaks* colouring, the Results
  view's scale) are peaks *so far*.
- A model whose `[OPTIONS]` StormSewer cannot parse shows the time reached
  but no progress fraction.
- Live results read the file, not the engine: nothing here slows the run.

## 23.3 The engine bridge

### What it is and why it is 32-bit

`runswmm` gives one answer at the end. Some things need the engine mid-run:
tight 1D-2D coupling ([§20.5](20-2d-overland.md)), where the 2D surface and
the 1D network exchange flow every routing step, and real-time control,
where a pump or gate setting is decided from live state rather than from
`[CONTROLS]` rules written in advance. EPA's engine has an API for exactly
that — `swmm_open`, `swmm_start`, `swmm_step`, `swmm_getValue`,
`swmm_setValue`, `swmm_end`, `swmm_close` in `swmm5.h` — but on Windows it
lives in `swmm5.dll`, which EPA ships as a **32-bit** DLL, and a 64-bit
process such as StormSewer cannot load a 32-bit DLL.

The engine bridge is the answer: `stormsewer-swmm-bridge32.exe`, a small
32-bit helper built from the `bridge/` crate, with no dependencies of its
own, that loads `swmm5.dll` and takes requests one line at a time on its
standard input. StormSewer starts it as a child process and talks to it
over pipes; the DLL runs unmodified, in its own process, and the `.rpt` and
`.out` it writes are byte-for-byte what a `runswmm` run of the same model
writes (the bridge is tested against that: same continuity errors, same
results file). A crash in the engine takes the helper down, not the app,
and a helper that stops answering for 60 seconds is killed and reported
rather than hanging the window.

### Where it lives

The app looks for the bridge in this order:

1. `STORMSEWER_SWMM_BRIDGE` — the full path to the helper, for a portable
   install or CI.
2. `stormsewer-swmm-bridge32.exe` beside `StormSewer.exe`. The installer
   and the portable zip put it there (and beside the `mesa\` copy of the
   app, which looks in its own folder).
3. In a debug build only, `target\i686-pc-windows-msvc\{release,debug}\`
   under the workspace, so a dev tree finds what `scripts\build-bridge.ps1`
   built.

The DLL is `swmm5.dll` beside the chosen engine's `runswmm.exe`
([§9.1](09-run-and-engines.md)). Without both, the features that need the
bridge are greyed with a note saying so; ordinary runs never need it.

### Building it

```powershell
scripts\build-bridge.ps1          # adds the i686 target if missing, builds, checks
scripts\build-bridge.ps1 -Copy    # and copies it beside target\release\StormSewer.exe
```

which is `cargo build -p stormsewer-swmm-bridge --release --target
i686-pc-windows-msvc` plus the same no-VC++-runtime check the app gets
(`.cargo\config.toml` links the C runtime statically for i686 as well). The
release workflow builds it on the Windows job, signs it with the app, and
stages it into the installer and the zip. On Linux and macOS the crate
compiles to a stub that prints `bridge needs Windows` and exits 2, so
`cargo build --workspace` is the same everywhere; its protocol tests run
against a fake engine on every platform. A 64-bit build of the helper
(plain `cargo build -p stormsewer-swmm-bridge`) compiles and runs but
refuses the DLL with `… is not a 64-bit DLL. EPA's swmm5.dll is 32-bit;
build this bridge for i686-pc-windows-msvc`.

### Protocol reference

`stormsewer-swmm-bridge32.exe <path to swmm5.dll>`; `--version` prints the
helper's version. One request per line on stdin, one reply per line on
stdout, flushed at once. Tokens are separated by spaces; a token holding a
space, a double quote or a backslash is written in double quotes with `\"`
and `\\` escapes (paths with spaces, object names with spaces). Numbers
are plain decimal; times are seconds of simulation time from the start.

Replies:

| Reply | Meaning |
|---|---|
| `ready bridge=<v> engine=<int> arch=<a>` | Printed once at start-up. `engine` is `swmm_getVersion()`, e.g. `52004` = 5.2.4. |
| `ok [key=value …]` | Done; fields as listed per request. |
| `done` | The run has reached its end (or failed earlier); only `end`/`finish`/`close` remain. |
| `err <code> <message>` | `code` is the engine's error number from `swmm_getError` (e.g. `303`), or `-1` for a protocol error (unknown name, bad syntax). The message is quoted. |
| `.. t=<s>` | Heartbeat, only during `until`, about every 2 s of wall clock. Not a reply; it says the engine is still stepping. |

Requests:

| Request | Reply |
|---|---|
| `info` | `ok bridge= engine= engine_version= arch= phase= duration_s=` |
| `open <inp> <rpt> <out>` | `swmm_open` + `swmm_start(1)`. `ok duration_s= routing_step_s= report_step_s= nodes= links= subcatch= gages= version= flow_units= start_days=`. `duration_s` is read from the model's `[OPTIONS]` (the API has no property for it); `start_days` is days since 1899-12-30. On an engine error the model is closed again. |
| `names node\|link\|subcatch\|gage` | `ok n=N` followed by N lines, one quoted name each, in engine index order. |
| `step` | One `swmm_step`: `ok t=<s>` or `done`. |
| `stride <s>` | `swmm_stride(s)`: advance a fixed number of seconds; the engine shortens its routing step to land on it. `ok t=` or `done`. |
| `until <s>` | Repeated `step` until the elapsed time reaches `<s>`: `ok t=` (the step that reached it) or `done`. Heartbeats meanwhile. |
| `get node <name> head\|depth\|volume\|latflow\|inflow\|overflow\|elev\|maxdepth\|type` | `ok v=<value>` (`swmm_getValue`, model units). |
| `get link <name> flow\|depth\|velocity\|setting\|topwidth\|length\|slope\|fulldepth\|fullflow\|timeopen\|timeclosed\|type` | `ok v=` |
| `get subcatch <name> rainfall\|runoff\|infil\|evap\|area` | `ok v=` |
| `get gage <name> rainfall` | `ok v=` |
| `get system elapsed\|current_date\|start_date\|route_step\|max_route_step\|report_step\|total_steps\|flow_units\|duration\|warnings` | `ok v=` (`elapsed` and `duration` in seconds) |
| `set node <name> latflow <q>` | Lateral inflow applied from now on (negative = withdrawal, capped by the engine at the stored volume). `ok` |
| `set node <name> head <h>` | Outfall stage from now on. `ok` |
| `set link <name> setting <x>` | Pump on/off, orifice or weir opening, 0–1. `ok` |
| `set gage <name> rainfall <i>` | Override a rain gage's intensity from now on. `ok` |
| `set system route_step <s>` | Change the routing step. `ok` |
| `end` | `swmm_end`: `ok runoff= flow= quality= warnings=` — the continuity errors in percent from `swmm_getMassBalErr`. |
| `report` | `swmm_report`: append the time-series tables to the `.rpt`. `ok` |
| `close` | `swmm_close` (calling `swmm_end` first if needed). `ok` |
| `finish` | `end` + `close` in one round trip: `ok runoff= flow= quality= warnings=`. `swmm_report` is *not* called, because `runswmm` skips it too whenever an `.out` path is given — that is what makes the `.rpt` identical. |
| `abort` | Ends and closes whatever is open, replies `ok`, exits. |
| `quit` | The same; end of input does the same without a reply. |

Every line the bridge prints is flushed before it reads the next request,
and requests are answered strictly in order. `stormsewer_swmm::bridge`
is the Rust client: `Session::open` runs the banner and `open`,
`step`/`step_until`/`stride`, the `node_*`/`link_*`/`set_*` accessors,
and `finish`, which returns the three continuity errors; dropping a
`Session` kills the helper.

### What depends on it

- **Tight 1D-2D coupling** ([§20.5](20-2d-overland.md)): the 2D solver
  needs the node heads every routing step and gives the engine the surface
  exchange back as lateral inflow. Loose coupling reads the `.out` after
  the run and does not need the bridge.
- **Real-time control** (planned): pump and gate settings decided during
  the run from live state, through `set link … setting`.
- **Live values** beyond what the `.out` holds — anything the run views do
  not report can be read with `get` at any step.

Without the bridge those features are unavailable and everything else in
this manual works exactly as before: ordinary runs, Stop and Live Results
use `runswmm` and the files it writes, not the DLL.
