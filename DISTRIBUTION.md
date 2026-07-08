# Getting StormSewer Into Users' Hands

A distribution plan grounded in what actually ships today — not what the README
once claimed. Companion to `READINESS.md` (which covers engineering readiness).

---

## 1. Who the first users are

Practicing **drainage / stormwater engineers** at small-to-mid civil firms and
municipalities, plus **students**. They currently use Autodesk Hydraflow Storm
Sewers, Bentley StormCAD, or HydroCAD. Characteristics that shape distribution:

- Windows-centric, often on **locked-down IT** (can't freely install .exe's).
- **Conservative and liability-bound** — they will not stamp a design on a tool
  they can't cross-check. Trust is the gate, not features.
- Already own project data in specific formats (Hydraflow `.STM`, LandXML, DXF).

## 2. The wedge: import + compare, don't ask them to switch

The fastest path to a first user is **not** "replace your tool." It is:

> "Import the Hydraflow project you already have, and see the same analysis —
> side by side."

We already have the two pieces this needs: a **`.STM` importer** and a
**validated engine**. A workflow of *import an existing project → run → show the
numbers match their current tool* turns our validation work into the sales pitch
and removes the trust barrier. This should be the headline of any demo.

## 3. Channels, ranked by adoption friction

| Channel | Friction | State today | What it needs |
| ------- | -------- | ----------- | ------------- |
| **Browser / web (WASM)** | Lowest — no install, no IT approval | **Built and working.** `stormsewer-wasm` compiles to wasm; `wasm/index.html` runs the full engine client-side; `pages.yml` deploys it | Enable GitHub Pages on the repo (workflow is ready) |
| **Windows installer (.exe)** | Medium — download + SmartScreen | Inno Setup script + `release.yml` pipeline ready; **untested on a runner**, unsigned | A tagged release run on a Windows runner, plus a **code-signing cert** (`CERT_PFX_BASE64` secret) |
| **GitHub Releases (zip + CLI)** | Low for technical users | `release.yml` builds + attaches Linux CLI, Windows installer, and web bundle on tag | Push a `v*` tag (`v0.7.0` ready) |
| **Cargo crate (`stormsewer`)** | Low for Rust devs | Publishable now (engine is clean) | `cargo publish` the engine lib |

The realistic sequence, now largely wired: **the web demo via GitHub Pages** as
the zero-friction wedge (just enable Pages), a **GitHub Release** on tag for the
CLI + web bundle + Windows installer, and the **signed Windows installer** for
mainstream engineers once a signing cert is in place.

## 4. Gap list to a first shippable release

Shippable-today (native): the app builds and runs; the engine is validated.
Blocking a *public* v0.7 release:

1. **Windows binary + signed installer.** Cross-build (or a Windows CI runner),
   a code-signing certificate (~$100–400/yr; without it, SmartScreen scares off
   non-technical users), and one real install test. The `.iss` is ready.
2. **Honest, first-run-friendly docs.** A one-page "open a project, run it,
   read the report" quick start, plus 2–3 realistic sample projects beyond the
   demo. (The in-app help topics exist; a standalone getting-started is thin.)
3. **A feedback loop.** An issue template (present) + a lightweight way to
   collect "the number differs from Hydraflow here" reports — these are gold for
   validation.
4. **Remove remaining aspirational claims** from user-facing text so nothing
   overpromises (WASM claim now corrected; audit the rest of README_APP etc.).

Not blocking, but the credibility multiplier: reproduce **one published HEC-22
example** (see `READINESS.md`) so the marketing claim "validated against FHWA
methods" is literally true.

## 5. The licensing / money tension (be clear-eyed)

- **Giving it away free to end users is fully compatible with GPL-3.0.** Users
  just run the software; copyleft only constrains *redistribution of modified
  source*. So direct-to-user free distribution has no licensing blocker.
- **Charging is allowed** (GPL doesn't forbid sale), but you must provide source
  and any recipient may redistribute — so a pure "sell the binary" model won't
  hold.
- **A closed-source "Pro" add-on linked into the GPL core is legally murky** and
  should not be assumed to work. The clean monetization paths are:
  - **Hosted web app (SaaS).** GPL does not trigger source distribution for
    network use (only AGPL would). A hosted version is the most defensible paid
    model and doubles as the low-friction channel from §3.
  - **Support, training, custom integration, validation/QA services.**
  - **Dual-licensing** — only if you own or can relicense 100% of the code.

Pick the model before charging anyone; it changes what you can build and how.

## 6. Recommended first move

Publish a **GitHub Release** (CLI + engine + sample projects + the validation
docs) and lead the README with the **import-your-Hydraflow-project** story. It
costs nothing, needs no certificate, puts the validated engine and the `.STM`
on-ramp in front of real users this week, and starts the feedback loop that
tells us which gap to close next.
