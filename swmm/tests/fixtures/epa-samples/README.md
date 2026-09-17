# EPA SWMM sample models

These `.inp` files are the example projects installed with EPA SWMM 5.2
(`Documents\EPA SWMM Projects\Samples`). They are US Government works in the
public domain, redistributed here unmodified as round-trip fixtures for
`stormsewer_swmm::doc`.

The files are CRLF, tab-separated, and carry the `[Polygons]` mixed-case
header and empty `[TAGS]` sections that the lossless reader is tested
against. `.gitattributes` marks them binary so no checkout normalises the
line endings — the tests assert byte-for-byte identity.
