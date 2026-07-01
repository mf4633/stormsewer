// SPDX-License-Identifier: GPL-3.0-or-later

//! `stormsewer-cli` — run a storm-sewer analysis from a `.ssn` network file.
//!
//! Usage:  stormsewer-cli [--size] [--review] <network-file>
//!
//! Flags:
//!   --size    Also run pipe sizing and print the sizing table.
//!   --review  Also run design review and print findings.
//!
//! See `stormsewer::parse` for the file format.

use std::process::exit;
use stormsewer::design::{
    design_review, format_design_review, format_sizing_table, size_network, DesignCriteria,
    ReviewCriteria,
};
use stormsewer::parse::parse_ssn;
use stormsewer::report::format_analysis;

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(1);
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    let size = if let Some(i) = args.iter().position(|a| a == "--size") {
        args.remove(i);
        true
    } else {
        false
    };
    let review = if let Some(i) = args.iter().position(|a| a == "--review") {
        args.remove(i);
        true
    } else {
        false
    };

    let path = match args.into_iter().next() {
        Some(p) => p,
        None => die("usage: stormsewer-cli [--size] [--review] <network-file>"),
    };

    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| die(&format!("cannot read {path}: {e}")));
    let parsed = parse_ssn(&text).unwrap_or_else(|e| die(&e));

    match parsed.network.analyze(&parsed.idf, &parsed.options) {
        Ok(a) => {
            print!("{}", format_analysis(&a));

            if size {
                let criteria = DesignCriteria::default();
                let recs = size_network(&parsed.network, &a, &criteria);
                print!("\n{}", format_sizing_table(&recs));
            }

            if review {
                let criteria = ReviewCriteria::default();
                let findings = design_review(&parsed.network, &a, &criteria);
                print!("\n{}", format_design_review(&findings));
            }
        }
        Err(e) => die(&e.to_string()),
    }
}
