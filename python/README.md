# stormsewer

Storm sewer hydrology and hydraulics for Python: Rational method, Manning,
standard-step HGL/EGL backwater, and HEC-22 inlets. The same Rust engine that
powers the [StormSewer desktop app](https://github.com/mf4633/stormsewer),
exposed as a native extension — no Python dependencies, no server.

```sh
pip install stormsewer
```

## Primitives

Float in, float out. Useful for replacing a spreadsheet column.

```python
import stormsewer as ss

ss.intensity(60, 10, 0.8, 12)          # IDF: i = a/(t+b)^c  ->  5.0607 in/hr
ss.rational_q(0.70, 5.0607, 1.0)       # Q = C i A           ->  3.5425 cfs
ss.manning_capacity(1.25, 0.013, 0.005)  # just-full capacity ->  4.5801 cfs
ss.normal_depth(3.5425, 1.25, 0.013, 0.005)  # -> 0.8254 ft (None if it won't fit)
ss.critical_depth(3.5425, 1.25)
ss.circular_geometry(1.25, 0.8254)
# {'area': 0.8597, 'wetted_perimeter': ..., 'hydraulic_radius': ..., 'top_width': ...}
```

Pass `si=True` for metric: the Manning factor becomes 1.0 and `g` becomes 9.81.

```python
ss.manning_capacity(0.4, 0.013, 0.005, si=True)   # m3/s
```

## Whole networks

```python
import stormsewer as ss
import pandas as pd

result = ss.analyze_project(ss.demo_project_json())

pipes = pd.DataFrame(result["pipes"])
print(pipes[["id", "design_q", "capacity", "pct_full", "velocity", "hgl_up"]])

nodes = pd.DataFrame(result["nodes"])
print(nodes[nodes.floods])          # anything surcharging to the surface
```

`analyze_file("project.ssproj")` reads a file saved by the desktop app, and
`analyze_ssn(text)` takes the plain-text network format:

```python
ss.analyze_ssn("""
IDF        60 10 0.8
TAILWATER  100.5
MINTC      10

NODE N1   inlet    0    0  104.0  110.0  1.0  0.70  12
NODE OUT  outfall  300  0  102.5  108.5

PIPE P1   N1  OUT  300  1.25  0.013
""")
```

Each pipe dict carries `id`, `from`, `to`, `slope`, `total_ca`, `tc`,
`travel_time`, `intensity`, `design_q`, `capacity`, `pct_full`, `velocity`,
`normal_depth`, `critical_depth`, `hgl_up`, `hgl_dn`, and `surcharged`. Each
node dict carries `id`, `tc`, `rim`, `hgl`, `freeboard`, and `floods`.

## Constants

`K_MANNING_US` (1.49), `K_MANNING_SI` (1.0), `G_US` (32.2), `G_SI` (9.81).

StormSewer uses **K = 1.49**, matching FHWA HDS-5 and HEC-22 rather than the
1.486 some spreadsheets use — a 0.27% difference in capacity. See
[VALIDATION.md](https://github.com/mf4633/stormsewer/blob/master/VALIDATION.md),
which works every number on a reference network by hand.

## What this is not

It computes the published equations; it does not decide whether the Rational
method suits your site, what C to use, or which storm to design for. Those stay
with the engineer sealing the drawing.

## License

GPL-3.0-or-later, like the rest of StormSewer.
