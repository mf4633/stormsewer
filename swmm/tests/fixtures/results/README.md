# Results fixtures

`Detention_Pond_Model.out` and `Detention_Pond_Model.rpt` are the binary
results and text report produced by running the unmodified EPA sample model
`../epa-samples/Detention_Pond_Model.inp` once through EPA SWMM.

- Engine: EPA SWMM 5.2 (Build 5.2.4), `runswmm.exe` from
  `C:\Program Files (x86)\EPA SWMM 5.2.4\`,
  SHA-256 `544e9f3da80577daf8f7beed8188d8d8788b3675cb49a82e8c71bef76a461c97`.
- Command: `runswmm.exe Detention_Pond_Model.inp Detention_Pond_Model.rpt Detention_Pond_Model.out`
  on a copy of the fixture, 2026-09-17. No input edits.
- Contents: 8 subcatchments, 14 nodes, 14 links, 144 reporting periods at
  300 s, flow units CFS, continuity −0.023 % (runoff) / 0.092 % (routing).

They are read by the `stormsewer_swmm` tests for the `.out` frame reader, the
`.rpt` summary-table parser, and the profile geometry, and by the app's
headless view tests. `.gitattributes` marks the directory binary: the report
carries a cp1252 superscript-3 in "1000 ft³" and must not be re-encoded or
have its line endings normalised.

Regenerate by running the same engine on the same input; any other build
produces a different report banner and the tests that pin `5.2.4` will say so.
