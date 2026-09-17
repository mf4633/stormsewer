# Appendix E. Credits and licences

StormSewer is GPL-3.0-or-later. Its engine is its own; its SWMM support
rests on public-domain work by the U.S. Environmental Protection Agency
and on the open-source projects below, which are credited here and not
copied. This manual, the docs under `docs/`, is part of the software and
carries the same licence.

## EPA SWMM

The Storm Water Management Model, its engine, its Windows GUI, its manuals
and its sample models are works of the United States Government and are in
the public domain. StormSewer runs the stock engine unmodified, cites the
manuals by section, redistributes the sample models unmodified as tutorial
datasets, and reproduces no manual text.

- *Storm Water Management Model User's Manual Version 5.2*, L. A. Rossman
  and M. A. Simon, EPA/600/R-22/030, 2022.
- *Storm Water Management Model Reference Manual Volume I — Hydrology
  (Revised)*, L. A. Rossman and W. C. Huber, EPA/600/R-15/162A, 2016.
- *Storm Water Management Model Reference Manual Volume II — Hydraulics*,
  L. A. Rossman, EPA/600/R-17/111, 2017.
- *Storm Water Management Model Reference Manual Volume III — Water
  Quality*, L. A. Rossman and W. C. Huber, EPA/600/R-16/093, 2016.
- *Storm Water Management Model Applications Manual*, J. Gironás,
  L. A. Roesner and J. Davis, EPA/600/R-09/077, 2009 — the model the
  tutorials' shape follows.
- Source: https://github.com/USEPA/Stormwater-Management-Model and
  https://github.com/USEPA/SWMM-GUI.

## Other public sources

- FHWA *Urban Drainage Design Manual*, Hydraulic Engineering Circular 22,
  3rd edition, FHWA-NHI-10-009, 2009 (S. A. Brown, J. D. Schall,
  J. L. Morris, C. L. Doherty, S. M. Stein, J. C. Warner) — inlets, gutter
  flow, access-hole losses.
- USDA-NRCS *National Engineering Handbook* Part 630, Chapter 4 — Storm
  Rainfall Depth and Distribution (210-630-H, 2019); NRCS TR-20 tabular
  rainfall distributions; NRCS TR-55 *Urban Hydrology for Small Watersheds*
  (1986) — the design-storm distributions and sheet-flow travel time.
- WSDOT *Highway Runoff Manual* M 31-16.04, Appendix 4C (2014) — the
  0.1-hour Type IA and II tables, used as a check.
- NOAA Atlas 14 Point Precipitation Frequency Estimates (Hydrometeorological
  Design Studies Center) — the PFDS csv the storm and IDF importers read.
- NOAA GHCN-Daily — the station csv the time-series importer reads.
- Z. P. Kirpich (1940); FAA *Airport Drainage* AC 150/5320-5B (1970);
  C. J. Keifer and H. H. Chu, "Synthetic storm pattern for drainage
  design", *J. Hydraulics Div. ASCE* 83(4), 1957; V. T. Chow, D. R. Maidment
  and L. W. Mays, *Applied Hydrology*, 1988.

## Open-source projects

Listed because StormSewer interoperates with them, learned from their
issue trackers, or follows their conventions. No code from any of them is
in StormSewer.

| Project | Licence | Relation |
| --- | --- | --- |
| [OWA-SWMM](https://github.com/pyswmm/Stormwater-Management-Model) | MIT | An alternate engine build that can be registered beside EPA's |
| [pyswmm](https://github.com/pyswmm/pyswmm) | BSD-2-Clause | Usable from the Python terminal; hotstart-at-time precedent |
| [swmmio](https://github.com/pyswmm/swmmio) | MIT | Usable from the Python terminal; its model-summary idea |
| [swmm_api](https://github.com/MarkusPic/swmm_api) | MIT | Its `.inp` coverage was a checklist for the schema |
| [swmmtoolbox](https://github.com/timcera/swmmtoolbox) | BSD-3-Clause | A reference for reading the `.out` |
| [openswmm.engine](https://github.com/HydroCouple/openswmm.engine) / [openswmm.gui](https://github.com/HydroCouple/openswmm.gui) | MIT / GPL-3.0 | Peer projects; their issue trackers informed the troubleshooting chapter |
| [generate_swmm_inp](https://github.com/Jannik-Schilling/generate_swmm_inp) (QGIS) | GPL-2.0 | Its layer and column conventions are the ones to follow for GIS interchange; interop by format only |
| [Giswater](https://github.com/Giswater/giswater_qgis_plugin) | GPL-3.0 | A GIS-first SWMM workflow reference |
| [GisToSWMM5](https://github.com/AaltoUrbanWater/GisToSWMM5) | MIT | Subcatchment generation reference |
| [swmmr](https://github.com/dleutnant/swmmr) | GPL-3 | Calibration reference for the Python cookbook's future |
| [SWMM5+](https://github.com/CIMM-ORG/SWMM5plus) | public domain | A possible future engine target |
| [SWMManywhere](https://github.com/ImperialCollegeLondon/SWMManywhere) | BSD-3-Clause | Synthetic network generation |
| [OpenSWMM](https://www.openswmm.org/) knowledge base | community posts | Threads cited in the troubleshooting chapter, paraphrased |
| [swmm5.org](https://swmm5.org/) (R. Dickinson) | author's copyright | Posts linked, not copied |

## StormSewer's own dependencies

The desktop app is built on `egui`/`eframe` (MIT or Apache-2.0), `rfd`,
`image`, `serde`, `serde_json`, `quick-xml`, `printpdf` and, on Windows,
`windows-sys` and Mesa's llvmpipe (MIT) for software rendering. SHA-256 is
implemented in the tree (FIPS 180-4) so the engine-hash claim does not
rest on a crate. The manual is built with pandoc (GPL) from Markdown.

## Trademarks

Hydraflow and Autodesk are trademarks of Autodesk, Inc. StormSewer is an
independent project, not affiliated with or endorsed by Autodesk, by CHI,
or by the U.S. EPA.
