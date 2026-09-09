# Directory and store listings

Copy for every venue StormSewer is listed on, kept here so the wording stays
consistent and honest across sites. Screenshots: `assets/screenshot.png` and
`packaging/listings/screenshots/`. Logo: `assets/icon/stormsewer-512.png`.

Facts every listing must keep straight:

- Free and open source (GPL-3.0). No paid tier in the desktop app.
- Windows, macOS, Linux; browser build of the engine only.
- Validated line by line against Hydraflow Storm Sewers on a real Civil 3D
  network (VALIDATION.md §8); say "compared", never "certified".
- Windows and macOS builds are unsigned (until the Store signs the MSIX).
- 0.9 series. Do not claim 1.0.

Links: https://github.com/mf4633/stormsewer · releases/latest ·
https://mf4633.github.io/stormsewer/ (browser build) ·
https://hydrocomplete.com/open-source

---

## Venue field limits (measured, not guessed)

SourceForge is much tighter than the copy below assumes. Checked 2026-09-09
while filling the listing in:

| Field | Cap |
|---|---|
| Short Summary | **70 chars** |
| Full Description | **1000 chars** |
| Features | separate repeatable field, one line each |

The 70-char summary in use is: `Free, open-source storm sewer design: Rational, Manning, HGL, HEC-22` (68).
The long description below has to be cut to about half for SourceForge; the
feature bullets go in the Features field, not the description.

SourceForge also has no Hydrology topic. The project is filed under
Scientific/Engineering > Mechanical and Civil Engineering and > CAD. There is
no plain "Graphical" user-interface trove either, only toolkit-specific ones,
so that category is deliberately left empty rather than filled with a wrong
toolkit.

## Short description (≤ 160 chars, all venues)

Free, open-source storm sewer design: Rational method, Manning, HGL/EGL backwater, HEC-22 inlets. Opens Civil 3D .stm, LandXML, DXF. Submittal PDF.

## Tagline (≤ 60 chars)

Storm sewer design you can read the source of.

## Long description (AlternativeTo, SourceForge, Capterra "About")

StormSewer is a free, open-source desktop program for gravity storm-drain
design and analysis. Draw the network on a scaled aerial or import it from a
Civil 3D Storm Sewers .stm file, LandXML or DXF, then run the Rational method,
Manning capacity for circular, box, elliptical and arch conduits, and a
standard-step HGL/EGL backwater pass with junction losses and tailwater.
HEC-22 inlet interception carries bypass from one inlet to the next.
Auto-sizing meets velocity and percent-full criteria. NOAA Atlas 14 import fits
your own IDF coefficients.

Reports come out as a submittal PDF with a title block on every page, ruled
pipe, structure and inlet schedules, a scaled plan, and a profile with real
elevation and station axes. HTML, CSV, LandXML and DXF export too.

The methods are public domain, so the implementation is readable: every
number in the report is worked by hand in VALIDATION.md, and the same network
was run through Hydraflow Storm Sewers side by side, with every difference
explained. Windows, macOS and Linux installers; a browser build of the engine;
Rust crate and Python package for scripting.

Written by a practicing water resources PE. GPL-3.0.

## Feature bullets (Capterra / SourceForge feature lists)

- Rational method flow accumulation down a dendritic network
- Manning partial-flow hydraulics: circular, box, elliptical, arch
- Standard-step gradually-varied HGL and EGL, junction losses, tailwater
- HEC-22 inlet interception (grate, curb, combination, sag) with bypass carryover
- Auto-sizing to velocity and percent-full criteria
- Tc by Kirpich, TR-55 sheet flow, or FAA, accumulated pipe by pipe
- NOAA Atlas 14 import with IDF fitting
- Import: Hydraflow / Civil 3D .stm, LandXML, DXF (network or site underlay)
- Export: submittal PDF, HTML, CSV, LandXML, DXF
- Design review: cover, freeboard, velocity, surcharge, adverse slopes
- Free, GPL-3.0; Windows, macOS, Linux; Rust crate; Python bindings

## Categories / tags

Civil engineering · Stormwater · Hydraulics · Hydrology · CAD add-on ·
Engineering design · Open source

## "Alternative to" (AlternativeTo)

Autodesk Hydraflow Storm Sewers · Bentley StormCAD · Hydrology Studio
Stormwater Studio · EPA SWMM (partial: gravity network design only)

## Microsoft Store listing

**Name:** StormSewer
**Category:** Developer tools → no; use *Productivity* › *Business* is wrong; the
Store category is **Utilities & tools** with *Engineering* not available; pick
**Productivity** and put "civil engineering" in the search terms.
**Search terms:** storm sewer, storm drain, stormwater, hydraulic grade line,
Rational method, Manning, HEC-22, Civil 3D, Hydraflow
**Age rating:** Everyone (IARC questionnaire: no content)
**Privacy policy URL:** https://hydrocomplete.com/privacy.html (the app makes
no network calls except the optional NOAA import the user starts)
**Support:** https://github.com/mf4633/stormsewer/issues
**Price:** Free
**Screenshots:** 1366×768 minimum; use the plan+report and profile captures.
**Package:** `scripts/build-msix.ps1 -IdentityName <reserved> -Publisher "<CN from Partner Center>"`

## What NOT to say anywhere

- "Certified", "approved", "validated by Autodesk". It was compared.
- Any PE seal, license number, or employer.
- "Replaces Hydraflow". It opens Hydraflow's files.
