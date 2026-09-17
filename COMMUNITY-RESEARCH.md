# SWMM community research for StormSewer (2026-09-17)

Read-only research. Sources: Reddit (via pullpush.io archive; reddit.com itself blocks fetches), OpenSWMM knowledge base (openswmm.org), GitHub issue trackers (USEPA, OWA/pyswmm, HydroCouple, swmmio, swmm-python), EPA manuals, CHI/PCSWMM public pages (feature headings + public release notes only; the CHI support KB is login-gated and was not read), Autodesk/Bentley public pages and forums, swmm5.org (R. Dickinson), Eng-Tips (blocked, search snippets only).

Rule observed: nothing below reproduces CHI documentation text or UI. PCSWMM mentions are limited to publicly stated feature headings and public release-note bug titles, used only to learn what users hit.

Reddit dates from pullpush are approximate (+/- a few weeks). Reddit permalinks are relative to https://www.reddit.com.

---

## A. Bugs / pain points people hit in SWMM front ends (ranked by frequency x severity)

| # | Pain point | What happens | Frequency / where | Sources |
|---|---|---|---|---|
| A1 | **Continuity error nobody understands; model "runs" but is wrong** | Users see 5-400%+ continuity error, negative error, or 0% with a failed run, and do not know if the model is valid. Top comment in a 2024 r/civilengineering thread: "Just because it runs does NOT mean the results are valid ... SWMM allows your model to 'silently fail'". Forum consensus thresholds: <1% excellent, <2% good, <5% acceptable, <0.5% for some. Causes: time step too large, short links, weirs on junctions with no surface area, storage with throttled outlet under EXTRAN surcharge, pumps. | Very high: 10+ OpenSWMM threads 2002-2021, 2 GitHub issues open, Reddit | https://www.openswmm.org/Topic/4223/interpreting-continuity-error ; https://www.openswmm.org/Topic/3925/high-continuity-error-in-flow-routing ; https://www.openswmm.org/Topic/1314/continuity-error-stability ; https://www.openswmm.org/Topic/2346/stability-problems ; https://www.openswmm.org/Topic/28820/node-continuity-errors-weirs ; https://www.openswmm.org/Topic/29632/swmm-version-5-1-015-run-was-unsuccessful-continuity-error ; https://www.openswmm.org/Topic/11479/meaning-of-negative-value-in-continuity-error ; https://www.openswmm.org/Topic/14273/storage-node-yielding-large-continuity-error ; https://github.com/USEPA/Stormwater-Management-Model/issues/32 ; https://github.com/USEPA/Stormwater-Management-Model/issues/210 ; https://www.reddit.com/r/civilengineering/comments/1oo11a1/comment/nn5nxxb/ ; https://www.reddit.com/r/civilengineering/comments/c320iu/xpswmm_modelling_speed/erouafb/ |
| A2 | **No GIS/CAD import or export in EPA SWMM; Civil 3D round trip is manual** | "EPA SWMM is a little infamous for not playing with other programs. Best you can do is upload a background image" (r/Hydrology, score 4). Workarounds: paste coordinates into the .inp, mapexport from Civil 3D, QGIS plugin, buy PCSWMM/SSA/SewerGEMS. Civil 3D -> SWMM .inp export exists only via SSA (File > Export > EPA SWMM5), and SSA is being retired. Bentley documents recurring SWMM import/export errors. | Very high: OpenSWMM 2001-2023, Reddit 2024-25, Autodesk forum, Bentley KB | https://www.openswmm.org/Topic/27047/importing-gis-data ; https://www.openswmm.org/Topic/27867/importing-cad-into-pcswmm ; https://www.openswmm.org/Topic/4532/importing-data-from-autocad ; https://www.openswmm.org/Topic/33811/how-to-connect-conduits-and-nodes ; https://www.reddit.com/r/Hydrology/comments/1u9w3hj/comment/osjgkim/ ; https://www.reddit.com/r/civilengineering/comments/1txie9f/comment/opw19f2/ (score 9: "tie into Civil3D for pipe storm network layouts and design?") ; https://forums.autodesk.com/t5/civil-3d-forum/storm-and-sanitary-analysis-ssa-import-from-epa-swmm-5-1/td-p/10877800 ; https://bentleysystems.service-now.com/community?id=kb_article_view&sysparm_article=KB0015080 ; https://www.mdpi.com/2073-4441/14/14/2262 |
| A3 | **Units: switching flow units converts nothing** | Changing CFS<->CMS/LPS changes only the label; inflow time series, baseline flows, pump/rating curves, weir Cd (3.3 stays 3.3 instead of 0.7), divider curves must be hand-converted. Manning n is the same in both systems (confuses people into reporting a bug). Weir Cd expected in metric form when in SI. PCSWMM had "Subcatchment width not updated after switching flow units" (7.3.3095). | High: 6 OpenSWMM threads 2007-2022; EPA Python UI #238 "project defaults don't handle metric units" | https://www.openswmm.org/Topic/4235/changing-flow-units ; https://openswmm.org/Topic/32845/how-to-unify-units ; https://www.openswmm.org/Topic/3620/swmm-5-11-picks-up-weir-cd-in-us-units-when-using-si-units ; https://www.openswmm.org/Topic/29947/5-1-013-bug-manning-roughness-of-conduits-not-converted-to-s-i-units-correctly ; https://www.openswmm.org/Topic/11436/cms-units-not-working ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/238 ; https://www.pcswmm.com/Downloads/PCSWMM (7.3.3095 note) |
| A4 | **Offsets: depth vs elevation, inverts, rim** | "The manual definition was unclear" (2007); users do not know offset = pipe invert - node invert; engine silently sets offset to 0 and raises rim elevation with only an .rpt warning; depth at a drop is shown as zero in profile. | High: OpenSWMM 2007-2017, swmm5.org, EPA GUI issue #5 | https://www.openswmm.org/Topic/3507/inlet-offset-and-outlet-offset-meaning ; https://www.openswmm.org/Topic/9489/conduit-depth-with-offsets ; https://www.openswmm.org/Topic/3858/conduit-length-shorter-than-elevation-drop ; https://swmm5.org/2013/08/23/what-node-and-link-invert-elevations-does-swmm-5-use/ ; https://github.com/USEPA/SWMM-GUI/issues/5 |
| A5 | **No undo** | Highest-scored reply (21) in the 2025 "Free GUI like PCSWMM" thread: "Please, please include an 'Undo' button!". EPA's Python port logged undo/redo broken for create/delete/move and for GIS layer add. | High salience, medium frequency | https://www.reddit.com/r/civilengineering/comments/1txie9f/comment/opw3cme/ ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/122 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/152 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/144 |
| A6 | **GUI crashes / access violations** | EPA GUI: comctl32.dll access violation on mouse-over (5 reports by 2018), startup access violation in 5.1.014, save fails with AV after converting a node type that anchors a map label, "Scrollbar property out of range" for >24,855-day runs, crash when LID units but no subcatchments. PCSWMM public notes: crashes importing time series from Excel, after large 2D runs, when first POI is a PDF. XPSWMM: "Software made in 1700s that crashes every time". | High across all products | https://www.openswmm.org/Topic/11386/swmm-crash ; https://www.openswmm.org/Topic/11413/swmm-access-violation-in-module-comctl32-dll ; https://www.openswmm.org/Topic/27062/access-violation-epaswmm5-exe ; https://github.com/USEPA/SWMM-GUI/issues/22 ; https://github.com/USEPA/SWMM-GUI/issues/4 ; https://www.epa.gov/system/files/other-files/2023-08/epaswmm5_updates.txt ; https://www.pcswmm.com/Downloads/PCSWMM ; https://www.reddit.com/r/civilengineering/comments/k3rk98b (via pullpush; XPSWMM) |
| A7 | **Lost work / silent file mutation on save or version change** | EPA 5.2.2 bug: backdrop disappears on reopen ("unfortunate bug that crept undetected"). "The SWMM5 GUI changed the input file slightly if it didn't like something." EPA Python UI: descriptions not saved to .inp, non-default infiltration methods not written. PCSWMM public notes: settings/objects lost when opening older versions, project .db missing after Save As on a network drive, erosion settings not saved on Save As. | Medium-high | https://www.openswmm.org/Topic/27275/how-to-import-a-georeferenced-swmm-backdrop-with-world-coordinates-from-an-arcgis-map-into-swmm ; https://www.openswmm.org/Topic/26296/swmm-dll-vs-gui ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/155 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/333 ; https://www.pcswmm.com/Downloads/PCSWMM (7.7.3920, 7.6.3695, 7.5.3406 notes) |
| A8 | **Profile plot limitations** | No max-HGL envelope (only snapshots at report steps, which miss the true peak); HGL discontinuous between node and conduit ("still a work in progress" - Rossman 2005); HGL clipped at rim unless ponding configured; zero depth drawn at drop structures; negative offsets mishandled (swmmio); HGL drawn under conduit with offset (PCSWMM fix 2025); missing ground line (EPA Python UI). | High: 2005-2025 | https://www.openswmm.org/Topic/9659/profile-plots-in-swmm-5-1 ; https://www.openswmm.org/Topic/2939/hgl-in-the-swmm5-profile-plot ; https://www.openswmm.org/Topic/15558/hydraulic-grade-line-above-junctions ; https://github.com/USEPA/SWMM-GUI/issues/5 ; https://github.com/pyswmm/swmmio/issues/204 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/365 ; https://www.pcswmm.com/Downloads/PCSWMM (7.7.3920) |
| A9 | **Backdrop georeferencing, map dimensions, auto-length** | World files, "Scale Map to Backdrop", auto-length only recomputed when a node moves, wrong lengths from paper-scale images, zeroed map dimensions, wanting WMS/ArcGIS REST imagery. | Medium-high: 6 OpenSWMM threads 2007-2020, Reddit 2024 | https://www.openswmm.org/Topic/27275/... (above) ; https://www.openswmm.org/Topic/3402/swmm-backdrop-and-auto-length ; https://openswmm.org/Topic/3087/problems-to-match-a-backdrop-image-to-an-existing-sewer-network-m-ap ; https://www.openswmm.org/Topic/4134/re-scaling-backdrop-map ; https://www.openswmm.org/Topic/3136/swmm5-back-drop-image-world-reference ; https://github.com/USEPA/SWMM-GUI/issues/9 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/187 ; https://www.reddit.com/r/gis/comments/1pt4o4m/ |
| A10 | **Results export (CSV/Excel/PDF) is painful** | Users copy from graph windows, use third-party NetSTORM / swmmtoolbox / custom tools that trip antivirus. GitHub #219 (2025): municipalities require a full report in the submittal; no way to generate graphs+tables in one document. SSA: "functionality is so pigeon-holed" that people export to spreadsheets. | High: 2008-2025 | https://www.openswmm.org/Topic/20948/export-swmm-output-results-to-excel ; https://www.openswmm.org/Topic/19891/export-datas-on-csv-files ; https://www.openswmm.org/Topic/11578/auto-export-select-results-to-excel ; https://www.openswmm.org/Topic/3187/saving-time-series-output-as-a-text-file ; https://swmm5.org/2014/03/06/how-do-you-export-a-time-series-from-swmm-5/ ; https://github.com/USEPA/Stormwater-Management-Model/issues/219 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/112 ; https://www.reddit.com/r/civil3d/comments/1pu5wno/comment/nvmli6x/ |
| A11 | **Batch runs / design-event scenarios / comparing runs** | 36 rainfall scenarios needed 36 .inp copies + R/sed scripting; comparing runs = text-diff of .rpt files; batch .bat files. | High: 2012-2020, plus new BatchSWMMRunner (2026) | https://www.openswmm.org/Topic/4432/automate-running-multiple-epa-swmm-input-files ; https://www.openswmm.org/Topic/26907/running-multiple-design-events ; https://www.openswmm.org/Topic/4769/differences-in-results-between-versions-of-swmm ; https://github.com/dickinsonre/BatchSWMMRunner |
| A12 | **Rain data: one NOAA depth -> hyetograph; file formats; ERROR 363** | "NOAA Atlas only provides a single value" -> users need a design-storm generator (Rossman's DSTORM add-in, 2022); NOAA temporal distributions produce lower peaks than SCS Type II (KC user, 2019); ERROR 363 invalid data in external file (Reddit 2015); GHCN station name >50 chars unreadable; DLL crash on long comment line in TS file; hundreds of near-identical [RAINGAGES] lines for radar rainfall. | High | https://www.openswmm.org/Topic/32894/rainfall-data ; https://www.openswmm.org/Topic/20994/how-to-create-100-yr-24-hour-time-series ; https://www.openswmm.org/Topic/23735/noaa-atlas-14-temporal-distribution ; https://www.openswmm.org/Topic/32275/dstorm-a-design-storm-wizard ; https://www.reddit.com/r/stormwater/comments/2wd0w4/rainfall_data_set_swmm/ ; https://github.com/USEPA/Stormwater-Management-Model/issues/224 ; https://github.com/USEPA/Stormwater-Management-Model/issues/165 ; https://github.com/USEPA/Stormwater-Management-Model/issues/164 |
| A13 | **ERROR 209 / cryptic engine errors; illegal .inp loads silently** | Undefined object from missing rain gage, outlet, divider link, or [REPORT] references to deleted objects; request to downgrade to a warning; Python UI loaded pipes with nonexistent end nodes without warning. | High: 7+ OpenSWMM threads | https://www.openswmm.org/Topic/16685/undefined-object-error-209 ; https://www.openswmm.org/Topic/31856/error-209 ; https://www.openswmm.org/Topic/13117/assigning-rain-gage-error-209 ; https://www.openswmm.org/Topic/15467/error-209-undefined-object-0-at-line ; https://github.com/USEPA/Stormwater-Management-Model/issues/171 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/92 |
| A14 | **Hotstart files: confusing, binary, buggy** | Users do not know how to create/use them; Error 335 mid-run (2025); hotstart node depths mismatch; want to save at specified times (added by pyswmm, later EPA); cannot inspect contents. | Medium-high | https://www.openswmm.org/Topic/19483/how-to-use-hotstart-file ; https://www.openswmm.org/Topic/4817/hotstart-file-issues ; https://www.openswmm.org/Topic/23562/read-and-edit-hot-start-files ; https://github.com/USEPA/Stormwater-Management-Model/issues/214 ; https://github.com/USEPA/Stormwater-Management-Model/issues/150 ; https://github.com/USEPA/Stormwater-Management-Model/issues/108 ; https://swmm5.org/2016/05/01/why-is-a-hot-start-file-important-in-swmm-5-and-infoswmm/ |
| A15 | **Instability diagnostics are truncated / unreadable** | .rpt lists only top 5 continuity/critical/instability locations; large numbers concatenate columns; run status did not show warnings or continuity error (Python UI). | Medium (but engine maintainers agree) | https://github.com/USEPA/Stormwater-Management-Model/issues/32 ; https://github.com/USEPA/Stormwater-Management-Model/issues/109 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/157 |
| A16 | **LID setup semantics** | "% of impervious area treated" vs "% of subcatchment"; area/width/imperviousness apply to non-LID portion only; LID hangs (5.1.013), 10x runtime with 49 LID subcatchments; rain-barrel "covered" increases continuity error. | Medium-high: 6+ threads, 4 GitHub issues | https://www.openswmm.org/Topic/4805/calculation-of-the-percent-of-impervious-area-treated-on-lid-usage-editor ; https://www.openswmm.org/Topic/9951/adjusting-subcatchment-parmeters-with-a-lid ; https://www.openswmm.org/Topic/4617/lid-percentage-area-and-surface-width-in-swmm5 ; https://www.openswmm.org/Topic/15573/lid--of-impervious-area-treated ; https://www.openswmm.org/Topic/5464/extremely-long-model-runtime ; https://github.com/USEPA/Stormwater-Management-Model/issues/16 ; https://github.com/USEPA/Stormwater-Management-Model/issues/166 |
| A17 | **Tidal outfalls: datum confusion** | Tide data in chart datum vs model in geodetic datum; user saw no effect because tide was 4,960 ft below outfall; advice to build a one-junction test model; hotstart for tidal initial condition. | Medium: 5+ threads | https://www.openswmm.org/Topic/17876/tidal-boundary-condition ; https://www.openswmm.org/Topic/2627/tidal-model ; https://www.openswmm.org/Topic/23586/how-to-define-an-outfall-with-a-rating-curve |
| A18 | **Pump curve types and control-rule syntax** | Type 2 pumps cycle on/off; users expected on/off depths; control rules fail with "clause invalid or out of sequence" because each IF needs its own RULE name. | Medium: 5+ threads 2004-2017 | https://www.openswmm.org/Topic/2773/pump-curves-in-swmm5 ; https://www.openswmm.org/Topic/9552/pump-control-rule-problem ; https://www.openswmm.org/Topic/4126/pump-station-control-rules ; https://www.openswmm.org/Topic/16749/pump-control-rules |
| A19 | **Dynamic-wave option and time-step guidance** | Which inertial term option, normal-flow limit, conduit lengthening, variable step; KW conversion errors (multiple outlets, adverse slopes); lengthening step 10-30 s gave 7x speed-up with negligible change. | Medium-high | https://www.openswmm.org/Topic/25086/kinematic-wave-vs-dynamic-wave-routing-question ; https://www.openswmm.org/Topic/3938/inertial-term-dampening ; https://swmm5.org/2017/02/05/dynamic-wave-routing-options-in-infoswmm-and-swmm5/ |
| A20 | **Surcharge vs flooding vs ponding semantics** | Nodes flagged surcharged while pipes <50% full (bad initial conditions under KW); HGL stops at rim unless Allow Ponding + ponded area set; surcharge depth meaning. | Medium: 8+ threads | https://www.openswmm.org/Topic/4224/node-surcharge-flooding-issue ; https://www.openswmm.org/Topic/29608/node-flooding-with-ponded-area ; https://openswmm.org/Topic/35307/node-flooding-and-surcharge ; https://www.openswmm.org/Topic/15558/hydraulic-grade-line-above-junctions |
| A21 | **Subcatchment width is a black art** | Manual wording changed between SWMM4 and 5; rules of thumb (area / max overland flow length, 2x longest pipe, skew factor); Reddit: "The width you're using in SWMM seems far too small"; Guo (2012): width is "a frequent source of user error". | High: 10+ threads | https://www.openswmm.org/Topic/3818/estimating-subcatchment-width ; https://www.openswmm.org/Topic/4862/estimating-width-for-a-subcatchment ; https://www.openswmm.org/Topic/11520/characteristic-width-of-subcatchments ; https://www.reddit.com/r/Hydrology/comments/1nlc7bp/comment/nf4m4ek/ ; https://www.openswmm.org/Topic/4426/swmm5-pcswmm-vs-other-models |
| A22 | **SWMM 5.2 streets/inlets confusion and bugs** | Inlets capture 100% until HGL reaches opening; inlets not separate nodes, max 5 per street; submergence not modeled; no warning for incompatible xsection/inlet pairs; curb inlet on grade efficiency wrong; depression units wrong in manual. | Medium (new feature) | https://www.openswmm.org/Topic/33879/inlet-capturing-in-swmm-5-2 ; https://www.openswmm.org/Topic/31636/epaswmm-5-2-released-feb-1-2022 ; https://github.com/USEPA/Stormwater-Management-Model/issues/139 ; https://github.com/USEPA/Stormwater-Management-Model/issues/104 ; https://github.com/USEPA/SWMM-GUI/issues/21 ; https://github.com/USEPA/Stormwater-Management-Model/issues/111 |
| A23 | **Results differ across engines/versions** | XPSWMM (SWMM4-based) vs SWMM5 900 cfs apart; 5.0.021 vs 5.1.007 evaporation/infiltration changed by bug fixes, threatening calibrated CSO models; pyswmm vs GUI time-series differences; DLL vs GUI 100% continuity. | Medium-high | https://www.openswmm.org/Topic/3867/discrepancies-between-xpswmm-and-epa-swmm-results ; https://www.openswmm.org/Topic/4769/differences-in-results-between-versions-of-swmm ; https://github.com/pyswmm/pyswmm/issues/538 ; https://github.com/pyswmm/pyswmm/issues/477 ; https://www.openswmm.org/Topic/26296/swmm-dll-vs-gui ; https://github.com/HydroCouple/openswmm.engine/issues/116 |
| A24 | **Price and licensing** | XPSWMM "$10-30k", "a very expensive license"; Autodesk moving XPSWMM users to ICM with named-user licences ("impossible to teach staff without individual ICM licenses"); PCSWMM "$6k/year", "only slightly above StormCAD"; InfoWater requires a full ArcGIS licence; Innovyze subscriptions/InfoCare ended 7 May 2025. | High on Reddit | https://www.reddit.com/r/civilengineering/comments/k9krvlc ; https://www.reddit.com/r/civilengineering/comments/jzlqu9q ; https://www.reddit.com/r/civilengineering/comments/jzkmxj0 ; https://www.reddit.com/r/Hydrology/comments/mwdcdfq ; https://www.reddit.com/r/civilengineering/comments/n75no3n (all via pullpush comment ids) ; https://www.autodesk.com/blogs/water/wp-content/uploads/sites/37/FY26-Autodesk-New-Water-Offerings-External-FAQ.pdf ; https://www.openswmm.org/Topic/2130/evaluation-of-dymanic-sewer-modeling-software |
| A25 | **EPA GUI is "clunky"/"archaic"; learning curve** | "SWMM ... is not a very user-friendly program ... that's why PCSWMM is popular"; "EPA SWMM is a free version that is extremely basic archaic software"; "not amazingly intuitive, but EPA-SWMM is free"; "Best advice I can give is to not self train. You'll pick up bad habits". | High | https://www.reddit.com/r/stormwater/comments/bvstkb/comment/epwgsyk/ ; https://www.reddit.com/r/civilengineering/comments/suvc3s/xpswmm_vs_epa_swmm/hxepypu/ ; https://www.eng-tips.com/threads/pc-swmm-vs-xp-swmm-vs-ddswmm.318639/ ; https://www.reddit.com/r/Hydrology/comments/1u9w3hj/comment/osjgkim/ |
| A26 | **Large models: GUI impractical, hard limits** | >254 .inp files per process (ERROR 305), 512 time-series files, 1,000-subcatchment models built from text not GUI, 15-200 h runtimes. | Medium | https://github.com/pyswmm/pyswmm/issues/353 ; https://github.com/USEPA/Stormwater-Management-Model/issues/197 ; https://www.openswmm.org/Topic/4597/modelling-large-catchments ; https://www.openswmm.org/Topic/5464/extremely-long-model-runtime |
| A27 | **Non-ASCII paths/filenames break the engine** | "õ, ä, ö, ü" in path fails; Chinese characters in path fail in SWMM6 GUI. | Medium (international users) | https://github.com/pyswmm/swmm-python/issues/71 ; https://github.com/pyswmm/swmm-python/issues/70 ; https://github.com/HydroCouple/openswmm.gui/issues/7 |
| A28 | **Engine geometry/physics bugs users must know about** | Custom elliptical pipe area/Rh wrong since SWMM4 (fixed 5.2.x); junction storage volume ignored (manholes under-represented); storage + throttled outlet under EXTRAN surcharge gives 200% error; flap gate losses. | Medium | https://github.com/USEPA/Stormwater-Management-Model/issues/144 ; https://github.com/USEPA/Stormwater-Management-Model/issues/200 ; https://github.com/USEPA/Stormwater-Management-Model/issues/210 ; https://github.com/USEPA/Stormwater-Management-Model/issues/201 |
| A29 | **Silent auto-corrections by the engine** | Pipe invert below node invert -> offset set to 0; crown above rim -> rim raised; only a warning line in the .rpt that users miss. | Medium | https://swmm5.org/2013/08/23/what-node-and-link-invert-elevations-does-swmm-5-use/ |
| A30 | **Orifice/outlet defaults differ from hand calcs** | PCSWMM orifice flow lower than hand calc; check entry/exit coefficients and tailwater. | Low-medium | https://www.reddit.com/r/civilengineering/comments/1ntv5aq/orifice_discharge_swmm/ |
| A31 | **Calibration has no home in EPA SWMM** | Users ask for observed-vs-simulated overlays, NSE, multi-gage strategy, auto-calibration; answers point to R (swmmr, RSWMM) or commercial tools. | Medium-high | https://www.openswmm.org/Topic/11733/guidance-on-model-calibration ; https://www.openswmm.org/Topic/23601/auto-calibration-of-pcswmm ; https://www.openswmm.org/Topic/3467/calibration-file-and-guidance ; https://www.reddit.com/r/Hydrology/comments/1qunaxw/ |
| A32 | **SSA retirement / gutter spread mismatches** | Municipalities reject SSA for "known issues" in flat terrain; AGN vs Hydraflow spread results disagree; users unsure whether to learn SSA, InfoDrainage, or something else. | Medium (Civil 3D users) | https://www.reddit.com/r/civil3d/comments/1pu5wno/ ; https://www.reddit.com/r/civilengineering/comments/1s9veeq/ |
| A33 | **Rational vs SWMM disagreement** | Peak flows from Rational vs SWMM/SCS differ 2x; root causes: Tc floor, width, runoff/rain time steps too coarse. | Medium (Reddit, 13 comments) | https://www.reddit.com/r/Hydrology/comments/1nlc7bp/ ; https://www.reddit.com/r/Hydrology/comments/1nqokml/ |
| A34 | **PCSWMM 2D is a "hack" (1D cells) and unstable** | "prone to going unstable if everything isn't just so", "finding out where the 30,000 m3/s was coming from ... outfall node being 2 mm above". Users move to HEC-RAS 2D. | Medium (Reddit 2024-25) | https://www.reddit.com/r/HECRAS/comments/1qqdftk/ ; https://www.reddit.com/r/Hydrology/comments/1qpkjy2/ |
| A35 | **Documentation gaps in the EPA manual itself** | [EVENTS] missing from Appendix D; [PROFILES]/[TAGS] undocumented in D.3; API guide variable count wrong; depression units wrong; CHM help being migrated. | Medium | https://github.com/USEPA/Stormwater-Management-Model/issues/96 ; https://github.com/USEPA/Stormwater-Management-Model/issues/111 ; https://github.com/USEPA/SWMM-GUI/issues/16 ; https://github.com/USEPA/Stormwater-Management-Model/issues/177 |
| A36 | **Tooling: Python wrappers hard to install, docs thin** | pyswmm: DLL load failures, macOS/arm64 install failures, "Please share more tutorials", "I want detailed variable and interface descriptions"; swmm-python 0.16.1 crashes Python. | Medium | https://github.com/pyswmm/pyswmm/issues/489 ; https://github.com/pyswmm/pyswmm/issues/557 ; https://github.com/pyswmm/pyswmm/issues/462 ; https://github.com/pyswmm/swmm-python/issues/155 ; https://github.com/pyswmm/swmm-python/issues/79 |

---

## B. Feature wishes (ranked by independent mentions)

| # | Wish | Independent mentions (approx.) | Sources |
|---|---|---|---|
| B1 | **Undo/redo** | 4 (Reddit top comment score 21; 3 EPA UI issues) | https://www.reddit.com/r/civilengineering/comments/1txie9f/comment/opw3cme/ ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/122 ; .../issues/152 ; .../issues/144 |
| B2 | **GIS/CAD/Civil 3D import-export (shapefile, GeoPackage, DXF, .inp <-> layers)** | 8+ (Reddit score 9 + 4; OpenSWMM 27047/27867/4532/33811; QGIS plugin paper; Autodesk forum) | see A2 ; https://plugins.qgis.org/plugins/generate_swmm_inp/ |
| B3 | **Batch/scenario runs and side-by-side comparison of runs/engines** | 6 (OpenSWMM 4432, 26907, 4769, 3867; BatchSWMMRunner; PCSWMM feature heading "scenario") | see A11, A23 |
| B4 | **Results export to CSV/Excel and a one-click submittal report (PDF)** | 7 (OpenSWMM 20948, 19891, 11578, 3187; GitHub #219; UI #112; Reddit nvmli6x) | see A10 |
| B5 | **Max-HGL envelope and correct HGL on profile plots; ground line; drop-structure depth** | 6 (9659, 2939, 15558; GUI #5; swmmio #204; UI #365) | see A8 |
| B6 | **Design-storm generator (NOAA Atlas 14 depth -> SCS/Huff/NRCS/Chicago/Nested hyetograph)** | 5 (32894, 20994, 23735, 32275, 27259) | https://www.openswmm.org/Topic/27259/creating-hypothetical-timeseries-based-on-idf-curves ; see A12 |
| B7 | **Better run diagnostics: full instability/continuity tables, per-node error magnitude, warnings surfaced in the UI, warnings instead of errors for stale [REPORT] entries** | 5 (GitHub #32, #109, #171; UI #157; Reddit "silently fail") | see A1, A13, A15 |
| B8 | **Real unit conversion when switching unit systems** | 3 (32845, 4235, UI #238) | see A3 |
| B9 | **Model QA summary: stranded nodes, connectivity, invert/offset sanity, missing coordinates** | 3 (swmmio #154; UI #92; 33811) | https://github.com/pyswmm/swmmio/issues/154 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/92 |
| B10 | **Cross-platform (Windows/Linux/macOS)** | 4 (Reddit opvxqwi; GitHub #131; UI #421; pyswmm mac issues) | https://www.reddit.com/r/civilengineering/comments/1txie9f/comment/opvxqwi/ ; https://github.com/USEPA/Stormwater-Management-Model/issues/131 ; https://github.com/USEPA/SWMM-EPANET_User_Interface/issues/421 |
| B11 | **Calibration: observed-data overlay, NSE/volume/peak stats, event isolation** | 5 (11733, 23601, 3467, 9576, Reddit 1qunaxw) | see A31 ; https://www.openswmm.org/Topic/9576/isolating-storm-events-in-observed-flow-data |
| B12 | **Automatic pipe sizing / design mode on the SWMM network** | 2 (OpenSWMM 3606 "some commercial wrappers offer this"; 2770) | https://www.openswmm.org/Topic/3606/automatic-pipe-sizing ; https://www.openswmm.org/Topic/2770/pipe-sizing-criteria |
| B13 | **One CSV/rain file feeding many gages / outfalls / inflows** | 2 GitHub (+ MRMS radar workflow) | https://github.com/USEPA/Stormwater-Management-Model/issues/164 ; https://github.com/USEPA/Stormwater-Management-Model/issues/31 |
| B14 | **Hotstart at specified times; inspect hotstart contents** | 3 (#150, 23562, GUI #12) | see A14 ; https://github.com/USEPA/SWMM-GUI/issues/12 |
| B15 | **Junction storage area attribute (manhole volume)** | 1 GitHub thread, 14 comments | https://github.com/USEPA/Stormwater-Management-Model/issues/200 |
| B16 | **Inlets as first-class map objects with own symbology; >5 inlets per street** | 1 (5.2 release thread) | https://www.openswmm.org/Topic/31636/epaswmm-5-2-released-feb-1-2022 |
| B17 | **Python scripting inside the front end** | 3 (Reddit nizqbs0 "QGIS & PCSWMM with python"; pyswmm #401 non-Python interface; PCSWMM public "PyForm" note) | https://www.reddit.com/r/civilengineering/comments/1o3xg5c/comment/nizqbs0/ ; https://github.com/pyswmm/pyswmm/issues/401 |
| B18 | **Tutorials / videos / worked examples** | 4 (pyswmm #489; Reddit nohehfv; 1u9w3hj thread; openswmm 3994) | https://github.com/pyswmm/pyswmm/issues/489 ; https://www.reddit.com/r/civilengineering/comments/1ov8jly/comment/nohehfv/ ; https://www.openswmm.org/Topic/3994/swmm-5-applications-manual-now-available |
| B19 | **Live results while the engine runs; HDF5/Arrow output** | 2 (GitHub #159, #189) | https://github.com/USEPA/Stormwater-Management-Model/issues/159 ; https://github.com/USEPA/Stormwater-Management-Model/issues/189 |
| B20 | **SWMM4 -> SWMM5 conversion recovering offsets/polygons** | 1 | https://www.openswmm.org/Topic/28890/swmm4-conversion |
| B21 | **Web/mobile/offline "all-in-one" front ends** | 6 self-promotional Reddit posts (HydroBOA), 0-2 score each - signal is weak | https://www.reddit.com/r/Hydrology/comments/1spmyji/ ; https://www.reddit.com/r/SaaS/comments/1rxx33e/ |
| B22 | **2D overland / dual drainage** | 4 Reddit threads, but users say PCSWMM 2D is a hack and prefer HEC-RAS 2D | see A34 |
| B23 | **Time-series import from Excel/CSV that does not crash** | PCSWMM fixed crashes for this in 7.6/7.7 - evidence it is a common path | https://www.pcswmm.com/Downloads/PCSWMM |
| B24 | **Report file readability (column spacing, scientific notation)** | 1 GitHub, 9 comments | https://github.com/USEPA/Stormwater-Management-Model/issues/109 |

What a "professional" SWMM front end is expected to do (feature headings from public pages, for expectation-setting only): GIS layer formats + projections + topology tools + DEM/watershed delineation + web basemaps; time-series management (edit, audit, gap-fill, stats, IDF, event analysis, scatter/calibration/duration plots); full SWMM5 + EPANET; sensitivity/calibration/error analysis; radar rainfall; output visualisation and reporting; scenarios; 1D-2D. Sources: https://www.pcswmm.com/Features ; https://www.autodesk.com/blogs/water/2025/04/23/xpswmm-vs-infoworks-icm-vs-infodrainage-which-solution-do-you-need/ ; https://www.autodesk.com/blogs/water/2023/03/10/what-xpswmm-users-who-made-the-switch-to-infoworks-icm-no-longer-need-to-worry-about/ (node limits, multi-user, stability, speed).

---

## C. Documentation: what people say works, what is missing, and a proposed StormSewer manual TOC

### What users praise
- **Tutorials with datasets, one per task, organised by modelling scenario** (HEC-RAS "Guides and Tutorials": Terrain, 1D steady, 1D unsteady, 2D, sediment, pipe networks, each with downloadable example data). https://www.hec.usace.army.mil/confluence/rasdocs/hgt/latest/tutorials
- **Worked examples that build on each other** (EPA SWMM Applications Manual: 9 examples on one watershed - post-development runoff, surface drainage hydraulics, detention pond design, LID, runoff quality, treatment, dual drainage, combined sewers, continuous simulation). https://www.chiwater.com/Files/Swmm_Apps_Manual.pdf ; https://www.sciencedirect.com/science/article/abs/pii/S1364815209002989
- **A quick-start tutorial as Chapter 2 of the user's manual** (EPA 5.2 manual: Ch.2 Quick Start Tutorial before Ch.3 Conceptual Model). https://www.epa.gov/system/files/documents/2022-04/swmm-users-manual-version-5.2.pdf
- **Theory separated from UI, with equations and references** (EPA Reference Manual Vol. I Hydrology: meteorology, surface runoff, infiltration (Horton, modified Horton, Green-Ampt, CN), groundwater, snowmelt; Vol. II Hydraulics: hydraulic model, dynamic wave (governing equations, solution, numerical stability), kinematic wave, cross-section geometry, pumps/regulators; Vol. III Water quality + LID). https://www.epa.gov/water-research/storm-water-management-model-swmm
- **Vendor tutorials and built-in training** ("PCSWMM has a lot of very good tutorials", "PCSWMM also has built in training"). https://www.reddit.com/r/civilengineering/comments/1ov8jly/comment/nohehfv/ ; https://www.reddit.com/r/Hydrology/comments/1u9w3hj/comment/oso4ykr/
- **Integrated docs across manual, help, tutorials and code examples** (Dickinson on the 5.2 inlets release). https://www.openswmm.org/Topic/31636/epaswmm-5-2-released-feb-1-2022
- **Reference docs that are unambiguous and visual** (SQLite: "best in the business", railroad diagrams "a perfect unambiguous explanation"). https://news.ycombinator.com/item?id=39604117 ; https://www.sqlite.org/docs.html
- **A book that teaches mental models, not just syntax** (Rust Book: "comprehensive and elaborate, yet friendly and thoughtful"). https://bitfieldconsulting.com/posts/best-rust-books
- **Separate documents for separate audiences** (QGIS: User Guide, Training Manual, Gentle Introduction to GIS, PyQGIS Cookbook, Developer Guide). https://docs.qgis.org/3.44/en/docs/index.html
- **Diátaxis**: tutorials / how-to / reference / explanation as four distinct forms (adopted by Canonical). https://diataxis.fr/ ; https://ubuntu.com/blog/diataxis-a-new-foundation-for-canonical-documentation

### What users find missing
- Guidance for rural/large catchments and width estimation ("new SWMM users need more guidance"). https://www.openswmm.org/Topic/4426/swmm5-pcswmm-vs-other-models
- Troubleshooting for continuity/instability beyond "lower the time step". A1 sources.
- Unit-switch checklist (what converts, what does not). A3 sources.
- Offsets/inverts explained with a diagram ("manual definition was unclear"). A4 sources.
- Datum guidance for tidal outfalls. A17.
- Variable/interface descriptions for scripting ("I want detailed variable and interface descriptions"). https://github.com/pyswmm/pyswmm/issues/557
- Appendix D omissions ([EVENTS], [PROFILES], [TAGS]). https://github.com/USEPA/Stormwater-Management-Model/issues/96
- Help/rainfall-manager docs in InfoDrainage ("ongoing challenges with help documentation"). https://www.reddit.com/r/civilengineering/comments/1s9veeq/comment/otlkz4n/
- Unintuitive loss-coefficient documentation in ICM ("This damn program is so unintuitive"). pullpush comment, r/civilengineering, InfoWorks search.

### Proposed StormSewer manual TOC (Diátaxis-shaped; EPA sections cited so each methods page points at public-domain theory)

**Part 0 - Start here**
0.1 Install (Windows/macOS/Linux), where engines live, version + hash stamping
0.2 Fifteen minutes to a running model (open Example1.inp, run, profile, results on map, undo something)
0.3 Reading the run status: continuity, warnings, critical elements (A1, A15)

**Part 1 - Tutorials (each with a downloadable dataset; mirrors EPA Applications Manual progression)**
1.1 First model from scratch: 6 subcatchments, 5 pipes, an outfall (EPA UM Ch.2)
1.2 Import a network from GIS/CAD (GeoPackage/shapefile/DXF/Civil 3D export) and fix topology (A2)
1.3 Design storms: NOAA Atlas 14 depth -> SCS/NRCS/Huff/Chicago hyetograph (A12)
1.4 Size a storm sewer with Rational/Manning/HEC-22, then verify with SWMM dynamic wave (B12)
1.5 Detention pond with orifice/weir outlet; hotstart for tidal outfall (A14, A17)
1.6 LID: bioretention + rain barrel, what the percentages mean (A16)
1.7 Continuous simulation with observed rainfall; compare runs and engines (A11, A23)
1.8 Calibrate against a flow gage: overlay, NSE, peak/volume/time-to-peak (A31)
1.9 Streets and inlets (5.2) (A22)
1.10 Python terminal: batch 36 design events and export CSV (A10, A11)

**Part 2 - Panes and tools (one chapter each; how-to style)**
2.1 Canvas: drawing, selection, snapping, vertices, backdrops and world files, map dimensions, auto-length, CRS (A9)
2.2 Property sheet and attribute grids: group edit, find/replace, units display
2.3 Layers pane: layer order, themes, labels, symbology, GIS overlays
2.4 Profile plot: HGL/rim/crown, max-HGL envelope, offsets and drops (A8)
2.5 Results on map: themes, animation, time slider, query
2.6 Plots and tables: time series, scatter, frequency; export CSV/PNG (A10)
2.7 Report viewer (.rpt): summaries, continuity, critical elements, instability index
2.8 Runner: engines, versions, hashes, parallel batch, logs, cancel
2.9 Compare: run vs run, engine vs engine, diff of .inp and results
2.10 Design tool: Rational/Manning/HEC-22 auto-size on the SWMM network
2.11 Model report: what goes into the submittal, PDF/DOCX/Markdown
2.12 Python terminal: API surface, cookbook link
2.13 Undo/redo, autosave, recovery, lossless .inp round-trip guarantees (A5, A7)

**Part 3 - Methods (explanation; equations + citations)**
3.1 SWMM object model and simulation loop (Ref. Man. Vol. I Ch.1)
3.2 Rainfall and climate (Vol. I Ch.2)
3.3 Surface runoff, nonlinear reservoir, width, time-step considerations (Vol. I Ch.3, esp. 3.5, 3.7, 3.8)
3.4 Infiltration: Horton, modified Horton, Green-Ampt, CN (Vol. I Ch.4)
3.5 Groundwater, snowmelt, evaporation (Vol. I Ch.5-7)
3.6 Flow routing: steady, kinematic wave, dynamic wave; numerical stability (Vol. II Ch.2-4, esp. 3.4 and 4.4)
3.7 Cross-sections, custom shapes, transects, storage geometry, critical/normal depth (Vol. II Ch.5)
3.8 Pumps, orifices, weirs, outlets; pump curve types; control rules (Vol. II Ch.6; UM Ch.3.3)
3.9 Water quality and LID (Vol. III)
3.10 Streets and inlets (UM 5.2 + Vol. II Addendum 2022)
3.11 StormSewer design methods: Rational, Manning, standard-step HGL, HEC-22 inlets (cite FHWA HEC-22)
3.12 Units: what SWMM converts internally and what it does not (A3; Vol. I 1.5)

**Part 4 - Troubleshooting (FAQ built from forum questions)**
4.1 Continuity error: meaning, thresholds, negative values, where it comes from (A1)
4.2 Instability: time step, conduit lengthening, inertial terms, short links, weirs on junctions, storage with throttled outlets (A1, A19)
4.3 Node is flooding/surcharged but pipes are half empty (A20)
4.4 Error 209 and friends: undefined objects, deleted references, [REPORT] leftovers (A13)
4.5 Missing coordinates / nothing on the map / wrong place after import (A9, B9)
4.6 Offsets, inverts, rim: what the engine silently changes (A4, A29)
4.7 Changed units and results changed (A3)
4.8 Tide has no effect: datums (A17)
4.9 Pump cycles on/off; control rule "clause invalid or out of sequence" (A18)
4.10 LID: 100% error, hang, 10x runtime (A16)
4.11 Rain file errors (363), GHCN names, long comment lines (A12)
4.12 Hotstart Error 335 and when to regenerate (A14)
4.13 Two engines disagree (A23)
4.14 Model is slow (A26)
4.15 Crash recovery and autosave (A6)
4.16 Non-ASCII paths (A27)

**Part 5 - File formats (reference)**
5.1 .inp section-by-section, including [EVENTS], [PROFILES], [TAGS], [STREETS], [INLETS], [INLET_USAGE] (fills EPA Appendix D gaps, A35)
5.2 .rpt structure; .out binary layout; hotstart (.hsf) layout
5.3 Rain/climate/time-series/calibration/interface file formats (UM Ch.11)
5.4 GIS interchange: GeoPackage/shapefile schema StormSewer reads and writes; DXF
5.5 StormSewer project/undo/run manifest formats

**Part 6 - Python terminal cookbook**
6.1 Open, edit, save lossless; 6.2 batch runs; 6.3 read results; 6.4 export CSV/plots; 6.5 sensitivity sweeps; 6.6 calibration loop; 6.7 compare engines; 6.8 pyswmm/swmmio interop

**Appendices**: units tables (UM App. A), parameter tables (Manning n, Horton, Green-Ampt, CN), error/warning code index, keyboard shortcuts, glossary (surcharge vs flooding vs ponding, offset depth vs elevation), credits and licences.

---

## D. Mapping to StormSewer

### Implement now (small, clearly scoped, high frequency)
- **Undo/redo everywhere incl. imports** (A5/B1) - already in scope; make GIS-layer add and group edits undoable (the EPA port missed those).
- **Run-status panel that surfaces continuity %, warnings, and the full (not top-5) critical-element/instability list** (A1, A15, B7) - parse the .rpt; show magnitude and % per node; link rows to map.
- **Unit-switch wizard** (A3/B8) - on FLOW_UNITS change, list every value SWMM does not convert (inflows, curves, weir Cd, DWF, divider curves) and offer to convert or leave.
- **Offset convention toggle with diagram + validation** (A4, A29) - show depth vs elevation both ways; flag pipe invert below node invert and crown above rim before the engine silently fixes them.
- **Pre-run model QA** (B9, A13) - undefined references, orphan nodes, missing coordinates, duplicate IDs (case), zero-length links, adverse slopes under KW, junction with only weir links.
- **Profile plot max-HGL envelope from .out (max over time) plus ground line, crown, rim, drop-structure depth at critical depth** (A8/B5).
- **Results export CSV for any object/variable set; copy table** (A10/B4).
- **Backdrop world-file loader + "scale map to backdrop" + CRS tag; auto-length that recomputes on demand** (A9).
- **Design-storm generator** (A12/B6): SCS/NRCS regional/Huff/Chicago/nested from a depth + duration; paste NOAA Atlas 14 CSV. Do not reuse DSTORM code (licence unstated); implement from public formulas.
- **Error-code index in-app** (A13): map SWMM error numbers to plain-language causes with links to the manual.
- **Non-ASCII safe paths** (A27): copy .inp to a temp ASCII path before invoking stock engines.
- **Autosave + crash recovery file** (A6, A7).

### Next
- **Batch/scenario runner + compare view** (A11/B3): one base model, N rain/tailwater variants, table of peaks/volumes/flooding per scenario per engine; diff .inp.
- **GIS import/export** (A2/B2): GeoPackage/shapefile read/write with the generate_swmm_inp column conventions (by convention only, not code); DXF layers -> nodes/links; Civil 3D via its SSA-style .inp export.
- **Calibration pane** (A31/B11): observed CSV overlay, NSE/RMSE/peak/volume/time-to-peak, event isolation.
- **Hotstart helper** (A14/B14): generate warm-up hotstart from base flows/tide; save at specified time (5.2.x feature); inspect header.
- **Submittal report** (B4): pick sections, graphs, tables -> PDF/DOCX; municipality-driven demand (GitHub #219).
- **Multi-gage CSV ingest** (B13): one CSV with many columns -> generated per-gage/outfall series files.
- **Junction->storage conversion helper** with manhole area (B15) - GUI-side only.
- **Inlet symbology as map objects** (B16) - GUI-side.
- **SWMM4 .dat -> 5 .inp** (B20) - low priority.

### Out of scope / never
- **2D overland solvers** (B22/A34) - users already distrust 1D-cell "2D"; StormSewer runs stock engines only.
- **Anything reading PCSWMM project files (.db, favorites, scenario DBs)** - proprietary format; also the CHI-copy prohibition.
- **Engine physics fixes** (A28: elliptical geometry, junction storage, EXTRAN surcharge) - report upstream; StormSewer stamps engine version+hash so users can see which engine produced which answer.
- **Auto-calibration/optimisation (SRTC-like)** - ship hooks in the Python terminal instead (swmmr/RSWMM/pyswmm exist).
- **HDF5/Arrow output, live-stepping API** (B19) - engine work.
- **Web/mobile builds** (B21) - weak signal, self-promotion.
- **Copying CHI tutorial text/structure verbatim** - structure ideas come from EPA (public domain) and Diátaxis.

---

## E. Open-source projects to credit or interoperate with

| Project | Licence | URL | Use for StormSewer | GPL-3.0-or-later compatibility |
|---|---|---|---|---|
| EPA SWMM 5.2 engine + Delphi GUI source | Public domain (US Gov work; README "released in the Public Domain") | https://github.com/USEPA/Stormwater-Management-Model ; https://github.com/USEPA/SWMM-GUI | Stock engine binaries; GUI behaviour reference (public domain) | Compatible |
| OWA-SWMM (pyswmm/Stormwater-Management-Model) | MIT | https://github.com/pyswmm/Stormwater-Management-Model | Alternate engine build with toolkit API | Compatible |
| openswmm.engine (SWMM 5.3 / SWMM 6, HydroCouple) | MIT now; relicensing to Apache-2.0 proposed Aug 2026 (issue #123); CLA required | https://github.com/HydroCouple/openswmm.engine | Multi-engine runner target; watch case-sensitivity and old-file parsing issues (#96, #110) | MIT and Apache-2.0 both compatible one-way into GPL-3 |
| openswmm.gui (Qt6 SWMM 6 GUI) | GPL-3.0 (LICENSE header) | https://github.com/HydroCouple/openswmm.gui | Peer project; study its issues (DST export bug, TS editor rounding, Chinese path) | Compatible |
| pyswmm | BSD-2-Clause | https://github.com/pyswmm/pyswmm | Python terminal interop, hotstart-at-time precedent | Compatible |
| swmmio | MIT | https://github.com/pyswmm/swmmio | .inp/.rpt dataframe conventions, profile plotter ideas, model-summary idea (#154) | Compatible |
| swmm_api (Markus Pichler) | MIT | https://github.com/MarkusPic/swmm_api | .inp read/write coverage checklist, GIS export | Compatible |
| swmmtoolbox (Tim Cera) | BSD-3-Clause | https://github.com/timcera/swmmtoolbox | .out reader reference | Compatible |
| generate_swmm_inp (QGIS plugin) | GPL-2.0 (LICENSE file is the GPL-2 text; "or later" wording not verified) | https://github.com/Jannik-Schilling/generate_swmm_inp | Follow its layer/column conventions so StormSewer GeoPackages open in QGIS; cite Schilling & Traenckner 2022 | **Conflict if GPL-2.0-only**: code cannot be copied into a GPL-3 project. File-format interop is fine. Verify header before any reuse. |
| Oslandia qgis-swmm | GPL-2.0 (migrated to GitLab) | https://github.com/Oslandia/qgis-swmm | Same caveat | Same caveat |
| Giswater QGIS plugin | GPL-3.0 | https://github.com/Giswater/giswater_qgis_plugin | GIS-first SWMM workflow reference | Compatible |
| GisToSWMM5 (Aalto) | MIT | https://github.com/AaltoUrbanWater/GisToSWMM5 | Automated subcatchment generation reference | Compatible |
| swmmr (R) | GPL-3 | https://github.com/dleutnant/swmmr | Auto-calibration vignette to cite in calibration chapter | Compatible |
| RSWMM | GPL-2.0 | https://github.com/PeterDSteinberg/RSWMM | Calibration reference only | Same GPL-2 caveat |
| SWMM5+ (CIMM) | Public domain / unencumbered | https://github.com/CIMM-ORG/SWMM5plus | Possible future engine target | Compatible |
| SWMManywhere | BSD-3-Clause | https://github.com/ImperialCollegeLondon/SWMManywhere | Synthetic network generation (docs/tutorial data) | Compatible |
| hydra (Rust, NEER AI) | AGPL-3.0 | https://github.com/neeraip/hydra | Rust hydraulics peer project | AGPL-3 can be combined with GPL-3 (GPLv3 s.13) but the AGPL network clause then applies to that part; prefer not to link |
| epanet-js | MIT for initial commit, FSL-1.1-MIT thereafter | https://github.com/epanet-js/epanet-js | Do not reuse post-fork code (FSL is not an open-source licence until its 2-year conversion) | Not compatible for recent code |
| swmm-js / swmmNode | Custom MIT-style ("freeware") text | https://github.com/ikegdivs/swmm-js ; https://github.com/swmm-js/swmmNode | Parser ideas only | Likely compatible; text is non-standard, check before copying |
| BatchSWMMRunner (Dickinson) | NOASSERTION | https://github.com/dickinsonre/BatchSWMMRunner | Batch-run UX reference only | Unknown - do not copy |
| DSTORM (Rossman) | Not stated (Windows binary) | https://sites.google.com/view/dstorm | Feature reference for design-storm generator; do not bundle | Unknown |
| NetSTORM | Freeware, licence unclear | https://dynsystem.com/netstorm | Export-tool reference only | Unknown |
| swmm5.org (Dickinson) | Blog text (copyright) | https://swmm5.org/ | Link, do not copy | n/a |
| OpenSWMM knowledge base (CHI-hosted, community posts) | Community posts; CHI hosts | https://www.openswmm.org/ | Cite thread URLs in FAQ; paraphrase, never copy | n/a |

Also note EPA's own recent bug lists for the manual's troubleshooting chapter: https://www.epa.gov/system/files/other-files/2022-03/swmm-5.2.0-updates-and-bug-fixes.txt and https://www.epa.gov/system/files/other-files/2023-08/epaswmm5_updates.txt (public domain).

---

## Method notes / caveats
- reddit.com and old.reddit.com refuse automated fetches; all Reddit content came from the pullpush.io archive API (submission/comment search). Some calls hit 429 so r/civilengineering coverage is partial; r/Stormwater, r/Hydrology, r/HECRAS, r/civil3d, r/gis were covered.
- Eng-Tips returns 403; only search-engine snippets were used from it (one quote in A25).
- CHI support KB (support.chiwater.com) is login-gated and was not read. Only pcswmm.com/Features headings and pcswmm.com/Downloads/PCSWMM release-note bug titles (public) were used.
- G2 review pages are CAPTCHA-gated; only search snippets used (ICM cost, no 3D).
- SWMM-USERS listserv archive (listserv.uoguelph.ca) mirrors OpenSWMM; not separately scanned.
