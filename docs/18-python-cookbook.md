# 18. Python terminal cookbook

Tools → Python Terminal… runs your own Python as a persistent child
process with three names bound. Type a block, click **Run** or press
`Ctrl+Enter`. A trailing expression is echoed the way a prompt echoes it,
and `_` holds it. **Restart Kernel** rebinds the names to the current run;
**Clear Output** empties the transcript.

## 18.1 What is bound

| Name | Value |
| --- | --- |
| `model` | the path of the model file the last run used, as a `str` — the model's own path, or the temp copy for an unsaved model — or `None` before any run |
| `out` | the path of the last run's `.out`, or `None` |
| `rpt` | the path of the last run's `.rpt`, or `None` |

They are strings, not objects: StormSewer does not impose a library on
you. Anything on your interpreter's path — `numpy`, `pandas`, `swmmio`,
`pyswmm`, `matplotlib` — is available. If ALR is configured, its folder is
on `sys.path` (`import quantum_hydraulics` works).

The banner on start names the interpreter and the three paths. Output is
UTF-8; stderr is folded into stdout so tracebacks appear in order; a
byte-order mark at the head of pasted code is stripped.

Nothing you do here changes the open model in the editor. To change the
model, write a new `.inp` and open it (File → Open .inp…), or edit in the
editor and re-run so the kernel sees the new file on restart.

## 18.2 Recipes

Every recipe below uses only the standard library unless it says
otherwise, and matches the `.out` layout in [§17.3](17-file-formats.md).

### 1. Read the header of the results file

```python
import struct

def out_header(path):
    with open(path, "rb") as f:
        f.seek(-24, 2)
        id_off, in_off, out_off, n_periods, err, magic = struct.unpack("<6i", f.read(24))
        assert magic == 516114522, "not a complete SWMM .out file"
        f.seek(0)
        magic, version, units, n_sub, n_node, n_link, n_poll = struct.unpack("<7i", f.read(28))
        f.seek(id_off)
        ids = {}
        for kind, n in (("sub", n_sub), ("node", n_node), ("link", n_link), ("poll", n_poll)):
            ids[kind] = []
            for _ in range(n):
                (ln,) = struct.unpack("<i", f.read(4))
                ids[kind].append(f.read(ln).decode("utf-8"))
        f.seek(out_off - 12)
        start_days, step_s = struct.unpack("<di", f.read(12))
    return dict(ids=ids, n_poll=n_poll, out_off=out_off, n_periods=n_periods,
                step_s=step_s, start_days=start_days, version=version, units=units)

h = out_header(out)
h["n_periods"], h["step_s"], h["ids"]["node"][:5]
```

### 2. One node's depth series

```python
def node_series(path, node, var=0):
    h = out_header(path)
    n_sub, n_node, n_link = (len(h["ids"][k]) for k in ("sub", "node", "link"))
    np_ = h["n_poll"]
    rec = 8 + 4 * (n_sub * (8 + np_) + n_node * (6 + np_) + n_link * (5 + np_) + 15)
    i = h["ids"]["node"].index(node)
    off_in_rec = 8 + 4 * n_sub * (8 + np_) + 4 * (i * (6 + np_) + var)
    xs = []
    with open(path, "rb") as f:
        for p in range(h["n_periods"]):
            f.seek(h["out_off"] + p * rec + off_in_rec)
            xs.append(struct.unpack("<f", f.read(4))[0])
    return [(p * h["step_s"] / 3600.0, x) for p, x in enumerate(xs)]

s = node_series(out, "SU1")          # var 0 = depth; 1 head, 2 volume, 3 lateral, 4 total inflow, 5 flooding
max(s, key=lambda t: t[1])
```

For links, replace `6 + np_` with `5 + np_` for the object stride, and the
offset within the record with `8 + 4*n_sub*(8+np_) + 4*n_node*(6+np_) +
4*(i*(5+np_) + var)`; link variables are 0 flow, 1 depth, 2 velocity, 3
volume, 4 capacity.

### 3. Continuity errors and warnings from the report

```python
import re
text = open(rpt, encoding="utf-8", errors="replace").read()
cont = re.findall(r"^\s*(\w[\w ]+?Continuity)[\s\S]*?Continuity Error \(%\) \.+\s+(-?[\d.]+)", text, re.M)
warns = [l.strip() for l in text.splitlines() if l.strip().startswith(("WARNING", "ERROR"))]
cont, warns
```

### 4. Run the engine yourself

```python
import subprocess, pathlib, os
engine = r"C:\Program Files (x86)\EPA SWMM 5.2.4\runswmm.exe"
inp = pathlib.Path(model)
rpt2, out2 = inp.with_suffix(".rpt"), inp.with_suffix(".out")
for p in (rpt2, out2):
    if p.exists(): p.unlink()          # runswmm appends to an existing report
subprocess.run([engine, str(inp), str(rpt2), str(out2)], check=False)
ok = "ERROR" not in open(rpt2).read() and out2.exists() and out2.stat().st_size > 0
ok
```

`runswmm` exits 0 whether it worked or not; test the report, as StormSewer
does. `stormsewer-swmm run model.inp` does all of this and exits non-zero
on failure.

### 5. Batch: thirty-six design storms

Vary the rain series in the text, run each copy, collect the peak at the
outfall. No parser needed: the `[TIMESERIES]` block is text.

```python
import re, shutil
base = open(model, encoding="utf-8").read()
storms = {"2yr": [...], "10yr": [...], "100yr": [...]}     # list of (h:mm, in/hr)
peaks = {}
for name, rows in storms.items():
    series = "\n".join(f"design\t\t{t}\t{v}" for t, v in rows)
    txt = re.sub(r"(\[TIMESERIES\][^\[]*)", lambda m: m.group(1).rstrip() + "\n" + series + "\n", base, count=1)
    txt = re.sub(r"(RainGage\s+\S+\s+\S+\s+\S+\s+TIMESERIES\s+)\S+", r"\1design", txt)
    p = pathlib.Path(model).with_name(f"batch_{name}.inp")
    p.write_text(txt, encoding="utf-8")
    subprocess.run([engine, str(p), str(p.with_suffix(".rpt")), str(p.with_suffix(".out"))])
    peaks[name] = max(v for _, v in node_series(str(p.with_suffix(".out")), "O2", var=4))
peaks
```

Adjust the regex to your gage's name and columns; check one generated file
by opening it in the editor before trusting thirty-six.

### 6. Compare two runs

```python
a = dict(node_series(out, "J11"))
b = dict(node_series(r"C:\path\to\other.out", "J11"))
max(abs(a[t] - b[t]) for t in a if t in b)
```

Results → Compare Runs… does this for every object's peak with the two
runs' engine hashes beside it.

### 7. Export every node's peak depth to CSV

```python
import csv
h = out_header(out)
with open("peaks.csv", "w", newline="") as f:
    w = csv.writer(f); w.writerow(["node", "max_depth", "t_hours"])
    for n in h["ids"]["node"]:
        t, d = max(node_series(out, n), key=lambda p: p[1])
        w.writerow([n, d, t])
```

### 8. With pandas and swmmio (if installed)

```python
import swmmio, pandas as pd
m = swmmio.Model(model)
m.inp.conduits.head()                       # the [CONDUITS] table as a DataFrame
m.inp.junctions.sort_values("InvertElev")
```

swmmio reads and writes `.inp` files with its own conventions; check the
diff before opening a file it wrote in StormSewer (StormSewer will read
it, but the spacing will be swmmio's).

### 9. With pyswmm: step through a run

```python
from pyswmm import Simulation, Nodes
with Simulation(model) as sim:
    su1 = Nodes(sim)["SU1"]
    for step in sim:
        pass
    su1.statistics
```

pyswmm binds its own copy of the engine (the OWA build); its version and
results may differ from the EPA binary StormSewer ran
([§16.13](16-troubleshooting.md)).

### 10. Check the run against ALR

```python
import subprocess, json, os
script = os.environ.get("STORMSEWER_ALR_SCRIPT")
r = subprocess.run(["python", script, out, "--json"], capture_output=True, text=True)
report = json.loads(r.stdout[r.stdout.index("{"): r.stdout.rindex("}") + 1])
[c for c in report.get("checks", []) if not c.get("passed", True)]
```

Tools → ALR Checks does this and shows the verdict in the panel.

## 18.3 Limits

- One block runs at a time; the terminal waits for it. A long block
  blocks the terminal, not the editor.
- No plots appear in the window; `matplotlib` opens its own window or
  writes a file.
- The kernel is a plain `exec` in one namespace with no history file;
  copy anything you want to keep out of the transcript.
- Closing the terminal window keeps the kernel; closing the app stops it.
