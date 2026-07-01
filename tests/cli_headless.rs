//! CLI subprocess tests — no GUI, exercises `stormsewer-cli` binary.

use std::path::PathBuf;
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cli_bin() -> PathBuf {
    manifest_dir()
        .join("target")
        .join("debug")
        .join(if cfg!(windows) {
            "stormsewer-cli.exe"
        } else {
            "stormsewer-cli"
        })
}

fn ensure_cli_built() {
    let status = Command::new("cargo")
        .args(["build", "--bin", "stormsewer-cli", "--quiet"])
        .current_dir(manifest_dir())
        .status()
        .expect("cargo build cli");
    assert!(status.success(), "failed to build stormsewer-cli");
}

#[test]
fn cli_sample_ssn_exits_zero_with_analysis_header() {
    ensure_cli_built();
    let sample = manifest_dir().join("examples").join("sample.ssn");
    let output = Command::new(cli_bin())
        .arg(&sample)
        .arg("--size")
        .arg("--review")
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("STORM SEWER ANALYSIS"));
    assert!(stdout.contains("PIPE SIZING TABLE"));
    assert!(stdout.contains("DESIGN REVIEW"));
    assert!(stdout.contains("P3"));
}

#[test]
fn cli_missing_file_exits_with_error() {
    ensure_cli_built();
    let output = Command::new(cli_bin())
        .arg("nonexistent_network_zz.ssn")
        .output()
        .expect("run cli");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error:"));
}

#[test]
fn cli_no_arguments_prints_usage() {
    ensure_cli_built();
    let output = Command::new(cli_bin()).output().expect("run cli");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("usage:"));
}