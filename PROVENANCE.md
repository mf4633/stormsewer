# Provenance & Clean-Room Statement

StormSewer is a free, open-source (**GPL-3.0-or-later**) re-creation of the
standard storm-sewer design methods, released for anyone to use, audit, and
extend. This document records where the implementation comes from.

> **How to read this file.** The *Methods*, *Clean-room basis*, and *File-format
> interoperability* sections are documented from the repository's own source and
> can be verified against it. The *Author attestation* section contains
> statements only the author(s) can make; those are marked and must be reviewed
> and confirmed by the author before this document is relied upon.

## License

GPL-3.0-or-later (full text in [`LICENSE`](LICENSE); SPDX headers in every source
file). This is a copyleft license: the software is free to use and modify, and
derivative works that are distributed must remain under the GPL. It is not
offered under any other license.

## Methods and their public sources

Every computational method implemented here is a long-established, publicly
published engineering formula in the public domain. None is proprietary to any
vendor.

| Method (source file) | Public reference |
| --- | --- |
| Rational peak flow `Q = C·i·A` (`network.rs`) | Kuichling (1889); standard hydrology texts |
| Manning open-channel flow `Q = (k/n)·A·R^⅔·√S` (`hydraulics.rs`) | Manning (1891); Chow, *Open-Channel Hydraulics* |
| Circular / rectangular / elliptical section geometry, normal & critical depth (`hydraulics.rs`) | Standard open-channel hydraulics; critical flow `Q²T = gA³` (Froude = 1) |
| HGL backwater with junction loss `H = K·V²/2g` (`network.rs`) | Standard energy-grade-line / minor-loss hydraulics |
| Time of concentration — Kirpich (`hydrology/tc.rs`) | Kirpich (1940) |
| Time of concentration — sheet flow (`hydrology/tc.rs`, `tr55.rs`) | NRCS **TR-55** (1986), Eq. 3-3 |
| Time of concentration — FAA (`hydrology/tc.rs`) | FAA airfield drainage method |
| Inlet interception (`design/inlets.rs`) — HEC-22 gutter-spread method: Izzard spread, frontal/side-flow efficiency, curb `L_T`, sag weir/orifice (splash-over velocity & clogging fraction are inputs) | FHWA **HEC-22** (Urban Drainage Design Manual), Chapter 4 — a public U.S. government document |
| IDF intensity `i = a/(t+b)^c` (`idf.rs`) | Standard IDF curve-fit form |

These are the same public methods used by every storm-sewer design tool. The
project's stated purpose is to make them **visible and auditable** — the source
and the analytical validation suites (`tests/validation.rs`,
`WORKED_EXAMPLE.md`, `tests/hgl_validation.rs`, `tests/units_si.rs`,
`tests/sections.rs`) show the formulas and check the numbers against hand
calculations.

## Clean-room basis

The implementation is written from the public formulas above and from
first-principles geometry (e.g. the elliptical wetted perimeter is derived by
numerical arc-length integration, not copied from a table). It does not
incorporate source code, algorithms, or data from any proprietary product.

## File-format interoperability

The importers/exporters exist so users can move their own data in and out; they
target documented or externally observable file formats:

- **Hydraflow `.STM`** (`io/stm.rs`) — a plain-text project format. The importer
  parses it field-by-field from the text structure (labels such as
  `"Line No. = "`, `"Invert Elev Up = "`, `"Return Period Index = "`).
  Reading a file format for interoperability is distinct from using a product's
  internal code.
- **LandXML** (`io/landxml.rs`) — the open LandXML 1.2 schema.
- **DXF** (`io/dxf.rs`) — the published Autodesk DXF interchange format.

## Author attestation

> *Affirmed by the author, whose statements these are. They concern facts about
> how the code was written, which the repository alone cannot prove.*

- [x] I authored this code independently, implementing the public methods
      cited above.
- [x] No source code, decompiled binaries, or proprietary technical
      documentation from Autodesk Hydraflow Storm Sewers (or any other
      commercial product) was used or copied.
- [x] The `.STM` import support was developed by inspecting the text file
      format, not from proprietary specifications or product internals.
- [x] To the best of my knowledge, this project infringes no third-party
      copyright.

Author: Michael Flynn   Date: 2026-07-08

## Trademarks & non-affiliation

*Hydraflow*, *Storm Sewers*, *AutoCAD*, and *Civil 3D* are trademarks of
Autodesk, Inc. *StormSewer* is an independent, community project and is **not
affiliated with, endorsed by, or sponsored by Autodesk**. Product names are used
only to describe interoperability and the standards being implemented.
