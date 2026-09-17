# Tutorial datasets

These `.inp` files are the example projects installed with EPA SWMM 5.2
(`Documents\EPA SWMM Projects\Samples`). They are works of the United States
Government and are in the public domain. They are redistributed here
unmodified so the manual's tutorials have something to open, and they are the
same files the lossless reader is tested against
(`swmm/tests/fixtures/epa-samples/`).

| File | What it exercises |
| --- | --- |
| `Detention_Pond_Model.inp` | 8 subcatchments, a storage unit with an orifice and a weir, kinematic wave. The manual's main tutorial model. |
| `Site_Drainage_Model.inp` | Post-development site drainage, dynamic wave. |
| `Inlet_Drains_Model.inp` | SWMM 5.2 streets and inlets (`[STREETS]`, `[INLETS]`, `[INLET_USAGE]`). |
| `Culvert_Model.inp` | A culvert with inlet control. |
| `Pump_Control_Model.inp` | A wet well, a pump, and control rules. |
| `Groundwater_Model.inp` | Aquifers and groundwater flow. |
| `LID_Model.inp` | LID controls and usage. |

The files are CRLF and tab-separated, and carry the mixed-case `[Polygons]`
header and empty `[TAGS]` sections that real files have. Keep them that way:
the round-trip tests assert byte-for-byte identity.
