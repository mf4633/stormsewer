# Appendix D. Glossary

Terms as this manual and the software use them. Where the engine defines a
term, the definition is the engine's.

**Alternating block** — A design-storm method that places the largest
incremental depth from an IDF curve at the storm's centre and the rest
alternately either side, so the storm reproduces the curve's average
intensity over every duration ([§15.7](15-methods.md)).

**Autosave** — A snapshot of a dirty model's text written every few
minutes to `<model>.inp.autosave`, offered for recovery on the next open,
removed on save ([§1.6](01-start-here.md)).

**Backdrop** — SWMM's own background image, placed in model coordinates by
`[BACKDROP] DIMENSIONS`, usually from a world file ([§3.9](03-map-and-tools.md)).
Distinct from an *underlay*.

**Continuity error** — The engine's mass-balance mismatch for a run,
reported per section as a percentage of inflow ([§16.1](16-troubleshooting.md)).

**Crown** — The top of a conduit's cross-section: invert plus rise.

**Design basis** — The IDF curve, return period, minimum Tc, junction K,
minimum slope and tailwater the storm-sewer design panel needs and a SWMM
model does not hold ([§12.1](12-design-panel.md)).

**Dirty** — A model with changes since it was last saved. The toolbar shows
`● Unsaved`.

**Engine** — EPA's `runswmm` executable, run as a child process, identified
by its reported version and the SHA-256 of the file ([chapter 9](09-run-and-engines.md)).

**Flooding** — Water leaving a node whose water level reached the rim
(plus `SurDepth`). Lost from the system unless ponding is on ([§16.4](16-troubleshooting.md)).

**Freeboard** — Rim elevation minus HGL at a structure. Negative freeboard
is surcharge to the surface.

**Gesture** — One map action from press to release; however many pointer
events it takes, it is one undo step.

**HGL** — Hydraulic grade line. In the profile, a node's invert plus its
reported depth at an instant; the *Max HGL* envelope is invert plus the
run's maximum depth ([chapter 11](11-profile.md)). In the design panel, the
standard-step water surface ([§15.4](15-methods.md)).

**Invert** — The bottom elevation of a node (its `Elevation`) or of a
conduit end (node invert plus offset).

**Lossless** — The document property that parsing a file and writing it
back yields identical bytes, and that an edit changes only the rows it
touched ([§1.6](01-start-here.md)).

**Offset (depth vs elevation)** — A link end's height above its node's
invert (`LINK_OFFSETS DEPTH`) or its absolute elevation (`ELEVATION`).
The keyword decides which; the numbers do not change with it
([§16.6](16-troubleshooting.md)).

**Ponding** — With `ALLOW_PONDING YES` and an `Aponded` area, flooded water
stays on the node in a pond of that area and drains back; the HGL can rise
above the rim ([§16.4](16-troubleshooting.md)).

**Reporting period** — One record in the `.out`, every `REPORT_STEP`. The
Time slider steps through them.

**Rim** — A node's ground elevation: invert plus `MaxDepth`, or the highest
connecting crown when `MaxDepth` is 0.

**Run Status** — The window that opens after a run with engine, hash,
continuity, diagnostics, warnings and errors ([§9.4](09-run-and-engines.md)).

**Scratch copy** — The copy of the model text the engine reads when the
model is dirty, unsaved, or on a non-ASCII path, under
`%TEMP%\StormSewer\run\<hash>\` ([§9.2](09-run-and-engines.md)).

**Section** — A `[HEADER]` block of the `.inp` and its rows ([chapter 17](17-file-formats.md)).

**Subcatchment width** — The width of the overland flow path; area divided
by the longest overland flow length ([§16.8](16-troubleshooting.md)).

**Surcharge** — A node's water level above the crown of its highest
connecting conduit: the pipe is under pressure ([§16.4](16-troubleshooting.md)).
In the design panel, a design flow above a pipe's open-channel capacity.

**Tc (time of concentration)** — In the design panel, the larger of a
pipe's inlet time and the upstream Tc plus travel time, floored at the
minimum Tc ([§15.3](15-methods.md)). SWMM has no Tc; its runoff timing
comes from the nonlinear reservoir and the width.

**Underlay** — The storm-sewer workspace's PNG or DXF drawing aid, not
stored in the `.inp` ([§7.3](07-layers.md)).

**Validation findings** — The live referential checks over the document,
errors (the engine would refuse or misread the model) and warnings
([§16.17](16-troubleshooting.md)).

**World file** — The six-line ESRI sidecar (`.pgw`, `.jgw`, `.wld`) that
places an image in map coordinates ([§17.7](17-file-formats.md)).
