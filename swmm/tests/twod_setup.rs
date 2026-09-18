// SPDX-License-Identifier: GPL-3.0-or-later

//! `Setup::build` against a real EPA sample model and a generated DEM:
//! units, node location and rims, warnings for nodes off the grid, rain
//! from the model's gage, sources from `[TIMESERIES]`, bank rasterising,
//! and the sidecar read from beside the model.

use std::fs;
use std::path::{Path, PathBuf};

use stormsewer_swmm::doc::InpDoc;
use stormsewer_swmm::gis::raster::Raster;
use stormsewer_swmm::twod::grid::{parse_time_seconds, RainInput};
use stormsewer_swmm::twod::{
    BankInterface, Config, InterfaceKind, NodeInterface, RainOnGrid, Roughness, Setup, Source,
};

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("stormsewer-twod-setup")
        .join(format!("{}-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn site_drainage(dir: &Path) -> (PathBuf, InpDoc) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/epa-samples/Site_Drainage_Model.inp");
    let model = dir.join("Site_Drainage_Model.inp");
    fs::copy(&src, &model).unwrap();
    let doc = InpDoc::read(&model).unwrap();
    (model, doc)
}

/// A gently sloping DEM over the model's coordinates (0..2000 both ways).
fn dem(dir: &Path, cell: f64) -> PathBuf {
    let n = (2000.0 / cell) as usize;
    let mut r = Raster::filled(n, n, 0.0, 0.0, cell, 0.0);
    for row in 0..n {
        for col in 0..n {
            let (x, y) = r.center(col, row);
            r.data[row * n + col] = 4960.0 + 0.005 * x + 0.003 * y;
        }
    }
    r.nodata = Some(-9999.0);
    let path = dir.join("dem.asc");
    r.write_asc(&path).unwrap();
    path
}

#[test]
fn setup_locates_nodes_reads_rain_and_sources_and_warns_off_grid() {
    let dir = workdir("locate");
    let (model, doc) = site_drainage(&dir);
    let dem_path = dem(&dir, 25.0);
    let first_conduit = doc.names("CONDUITS")[0].clone();
    let config = Config {
        dem: Some(PathBuf::from("dem.asc")), // relative to the model
        // Crop so J2 (x ≈ 1221) falls outside.
        window: Some((300.0, 800.0, 1000.0, 1100.0)),
        rain: RainOnGrid::Gage("RainGage".into()),
        sources: vec![Source {
            name: "hose".into(),
            x: 500.0,
            y: 900.0,
            series: "2-yr".into(),
        }],
        nodes: vec![NodeInterface {
            node: "J1".into(),
            kind: InterfaceKind::Inlet {
                perimeter: 6.0,
                area: 1.5,
            },
            weir_coeff: Some(0.55),
            lid_open: false,
        }],
        banks: vec![BankInterface {
            link: first_conduit.clone(),
            right: false,
            polyline: Vec::new(), // from the link's own geometry
            crest: None,
            weir_coeff: None,
        }],
        sealed: vec!["J3".into()],
        ..Config::default()
    };
    let _ = &dem_path;
    let setup = Setup::build(&model, &doc, &config).unwrap();
    assert!(!setup.metric, "FLOW_UNITS CFS");
    assert_eq!(setup.dem.cell, 25.0);
    assert_eq!((setup.dem.ncols, setup.dem.nrows), (28, 12));
    assert_eq!(setup.manning.data[0], 0.05);
    // J1 is an inlet with its own coefficient; J3 sealed; J2 warned.
    let j1 = setup.nodes.iter().find(|n| n.interface.node == "J1").unwrap();
    assert_eq!(j1.interface.kind, InterfaceKind::Inlet { perimeter: 6.0, area: 1.5 });
    assert_eq!(j1.interface.weir_coeff, Some(0.55));
    // J1: invert 4973, MaxDepth 0, so the engine raises its full depth to
    // the crown of C1 (offset 0 + 3 ft). EPA link.c link_validate.
    assert_eq!(j1.rim, 4976.0, "invert + the engine's full depth");
    let (x, y) = setup.dem.center(j1.col, j1.row);
    assert!((x - 648.532).abs() < 25.0 && (y - 1043.713).abs() < 25.0);
    assert!((j1.ground - setup.dem.sample(648.532, 1043.713).unwrap()).abs() < 1e-9);
    let j3 = setup.nodes.iter().find(|n| n.interface.node == "J3").unwrap();
    assert_eq!(j3.interface.kind, InterfaceKind::Sealed);
    assert!(setup.nodes.iter().all(|n| n.interface.node != "J2"));
    assert!(setup.warnings.iter().any(|w| w.contains("J2") && w.contains("outside")), "{:?}", setup.warnings);
    // The outfall is a sink at its invert.
    let o1 = setup.nodes.iter().position(|n| n.interface.node == "O1");
    if let Some(i) = o1 {
        let stage = setup.inputs.outfalls.iter().find(|o| o.node == i).unwrap().stage;
        assert_eq!(stage, 4962.0);
    }
    // Rain: INTENSITY gage at 0:05 → the first value 0.29 in/hr, held 300 s.
    match &setup.inputs.rain {
        RainInput::Series { series, interval_s } => {
            assert_eq!(*interval_s, 300.0);
            let expect = 0.29 / 12.0 / 3600.0;
            assert!((series.v[0] - expect).abs() < 1e-12);
            assert!((setup.inputs.rain.rate_at(100.0) - expect).abs() < 1e-12);
            assert!((setup.inputs.rain.rate_at(301.0) - 0.33 / 12.0 / 3600.0).abs() < 1e-12);
        }
        other => panic!("rain not resolved: {other:?}"),
    }
    // Source series in ft³/s (CFS factor 1), located on the grid.
    assert_eq!(setup.inputs.sources.len(), 1);
    assert_eq!(setup.inputs.sources[0].series.v[0], 0.29);
    assert_eq!(setup.inputs.sources[0].series.t[1], 300.0);
    // The bank followed the conduit's own polyline.
    assert_eq!(setup.banks.len(), 1);
    assert!(!setup.banks[0].cells.is_empty());
    assert_eq!(setup.banks[0].interface.link, first_conduit);
    assert!(setup.banks[0].cells.iter().all(|(_, _, crest)| *crest > 4960.0), "crest from the DEM");
    // Model clock: 0:00 for START_TIME, duration from END_DATE/END_TIME.
    assert!(setup.inputs.model_duration_s > 0.0);
    assert_eq!(parse_time_seconds("0:05"), Some(300.0));
}

#[test]
fn setup_resamples_reads_a_roughness_grid_and_a_class_table() {
    let dir = workdir("resample");
    let (model, doc) = site_drainage(&dir);
    dem(&dir, 25.0);
    // A roughness grid with a hole and a land-cover raster.
    let mut n_grid = Raster::filled(20, 20, 0.0, 0.0, 100.0, 0.02);
    n_grid.data[0] = f64::NAN;
    n_grid.write_asc(&dir.join("n.asc")).unwrap();
    let mut classes = Raster::filled(20, 20, 0.0, 0.0, 100.0, 11.0);
    classes.data[399] = 99.0; // unknown class at the bottom-right
    classes.write_asc(&dir.join("lc.asc")).unwrap();

    let config = Config {
        dem: Some(dir.join("dem.asc")),
        cell: Some(50.0),
        roughness: Roughness::Grid(PathBuf::from("n.asc")),
        ..Config::default()
    };
    let setup = Setup::build(&model, &doc, &config).unwrap();
    assert_eq!(setup.dem.cell, 50.0);
    assert_eq!((setup.dem.ncols, setup.dem.nrows), (40, 40));
    assert_eq!(setup.manning.data[5], 0.02);
    assert_eq!(setup.manning.data[0], 0.05, "hole filled with the default");
    assert!(setup.warnings.iter().any(|w| w.contains("roughness grid")));

    let config = Config {
        dem: Some(dir.join("dem.asc")),
        roughness: Roughness::Classes {
            raster: PathBuf::from("lc.asc"),
            table: vec![(11, 0.03)],
        },
        ..Config::default()
    };
    let setup = Setup::build(&model, &doc, &config).unwrap();
    assert_eq!(setup.manning.data[0], 0.03);
    let last = setup.manning.data.len() - 1;
    assert_eq!(setup.manning.data[last], 0.05, "unknown class → default");
    assert!(setup.warnings.iter().any(|w| w.contains("class")));

    // Missing DEM and bad run settings are errors, not panics.
    assert!(Setup::build(&model, &doc, &Config::default()).is_err());
    let bad = Config {
        dem: Some(dir.join("dem.asc")),
        courant: 2.0,
        ..Config::default()
    };
    assert!(Setup::build(&model, &doc, &bad).is_err());
    let missing = Config {
        dem: Some(dir.join("nope.asc")),
        ..Config::default()
    };
    assert!(Setup::build(&model, &doc, &missing).is_err());
}

#[test]
fn sidecar_beside_the_model_drives_the_setup() {
    let dir = workdir("sidecar");
    let (model, doc) = site_drainage(&dir);
    dem(&dir, 40.0);
    let side = Config::sidecar_path(&model);
    fs::write(
        &side,
        "; test sidecar\n[GRID]\nDEM dem.asc\n[ROUGHNESS]\nUNIFORM 0.04\n[RAIN]\nCONSTANT 2.0\n[RUN]\nDURATION 600\nOUTPUT_STEP 60\n[SEALED]\nJ4\n",
    )
    .unwrap();
    let config = Config::read(&side).unwrap();
    assert_eq!(config.rain, RainOnGrid::Constant(2.0));
    let setup = Setup::build(&model, &doc, &config).unwrap();
    assert_eq!(setup.manning.data[0], 0.04);
    let j4 = setup.nodes.iter().find(|n| n.interface.node == "J4").unwrap();
    assert_eq!(j4.interface.kind, InterfaceKind::Sealed);
    // 2 in/hr on the grid.
    assert!((setup.inputs.rain.rate_at(0.0) - 2.0 / 12.0 / 3600.0).abs() < 1e-15);
    let summary = stormsewer_swmm::twod::run(&setup, &mut |_| true).unwrap();
    assert_eq!(summary.frames, 11);
    assert!(summary.inflow > 0.0);
    assert!(summary.mass_error_pct.abs() < 1e-6, "{}", summary.mass_error_pct);
}
