# Appendix C. Error codes

The engine reports problems as `ERROR nnn: …` (fatal: the run stops) and
`WARNING nn: …` (the run continues, with something changed) in the
`.rpt`. The authoritative list is the EPA SWMM 5.2 User's Manual,
Appendix E, and the engine's `error.c`.

StormSewer carries an index of all 112 codes — 11 warnings and 101 errors
— each with what it means in plain language, what usually causes it, and
what to do about it, with the remedies taken from what the OpenSWMM and
GitHub threads show people needed. The index is in one place in the
source, `swmm/src/errors.rs`, and is not duplicated here; two things in
the app read it:

- **Help → SWMM Error Codes…** lists the whole index with a **Search**
  box (by number, by subject, or by a word from the message).
- **Results → Run Status…** shows every `WARNING` and `ERROR` line of the
  last run with its entry from the index and, where the message names an
  object, a button that selects it on the map ([§9.4](09-run-and-engines.md)).

The codes the troubleshooting chapter discusses by name:

| Code | Subject | See |
| --- | --- | --- |
| WARNING 03 | a negative link offset set to zero | [§16.6](16-troubleshooting.md) |
| WARNING 06 | dry-weather time step raised to the wet-weather step | [§16.17](16-troubleshooting.md) |
| WARNING 09 | time-series interval greater than the recording interval | [§16.10](16-troubleshooting.md) |
| ERROR 111 | invalid (zero or negative) conduit length | [§16.5](16-troubleshooting.md) |
| ERROR 141 | an outfall with more than one link, or an outgoing link | [§16.5](16-troubleshooting.md) |
| ERROR 191 | simulation ends before it starts | [§16.5](16-troubleshooting.md) |
| ERROR 195 | report step shorter than the routing step | [§16.5](16-troubleshooting.md) |
| ERROR 209 | undefined object (a deleted node, gage, curve or series still referenced) | [§16.5](16-troubleshooting.md) |
| ERROR 211 | invalid number (zero or negative area, and the like) | [§16.5](16-troubleshooting.md) |
| ERROR 2xx control-rule errors | a rule clause invalid or out of sequence | [§16.9](16-troubleshooting.md) |
| ERROR 335 | error reading the hotstart file | [§16.11](16-troubleshooting.md) |
| ERROR 363 | invalid data in a rain or time-series file | [§16.10](16-troubleshooting.md) |

The validator ([§16.17](16-troubleshooting.md)) names the engine's code
in its own message where it catches the same condition before the run
(`length is zero or negative (ERROR 111)`), so the number is the same in
the findings list, the Run Status window and the index.
