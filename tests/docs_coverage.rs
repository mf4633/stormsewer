// SPDX-License-Identifier: GPL-3.0-or-later

//! The manual covers the SWMM editor's menus, toolbar and dialogs.
//!
//! Every menu label, dialog title and toolbar tool name found in the
//! editor's chrome sources must appear somewhere in `docs/*.md`, so a new
//! feature shipped without a line of documentation fails CI. Modelled on
//! `app/src/ui_tests.rs::menu_inventory_is_covered`, but the target is the
//! manual rather than a hard-coded list: the label text itself is what has
//! to be present in the docs.
//!
//! Source files are read as text, so this test compiles without the app
//! crate's GUI dependencies.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The label inside the first quoted string after `pat` on `line`, if any.
fn quoted_after<'a>(line: &'a str, pat: &str) -> Option<&'a str> {
    let i = line.find(pat)?;
    let rest = &line[i + pat.len()..];
    // Builder calls may put whitespace before the string.
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let j = rest.find('"')?;
    Some(&rest[..j])
}

/// Labels from the widget constructors that carry user-visible text.
fn labels_in(src: &str) -> Vec<String> {
    const PATTERNS: &[&str] = &[
        ".button(",
        "egui::Button::new(",
        "Button::new(",
        "menu_button(",
        "Window::new(",
        "window(ctx,",
    ];
    let mut out = Vec::new();
    for line in src.lines() {
        let l = line.trim();
        for pat in PATTERNS {
            if let Some(label) = quoted_after(l, pat) {
                out.push(label.to_string());
            }
        }
        // `checkbox(&mut x, "Label")` and `selectable_label(cond, "Label")`
        // carry the text as the second argument.
        for pat in ["checkbox(", "selectable_label("] {
            if let Some(i) = l.find(pat) {
                let rest = &l[i + pat.len()..];
                if let Some(c) = rest.find(',') {
                    if let Some(label) = quoted_after(&rest[c + 1..], "") {
                        out.push(label.to_string());
                    }
                }
            }
        }
    }
    out
}

/// The toolbar tool names: the `short()` labels in `swmm_tools.rs`.
fn tool_names(src: &str) -> Vec<String> {
    let start = src.find("pub fn short(").expect("SwmmTool::short exists");
    let body = &src[start..];
    let end = body[1..].find("pub fn ").map(|i| i + 1).unwrap_or(body.len());
    body[..end]
        .lines()
        .filter(|l| l.contains("=> \""))
        .filter_map(|l| quoted_after(l, "=> "))
        .map(str::to_string)
        .collect()
}

fn docs_text() -> String {
    let dir = root().join("docs");
    let mut text = String::new();
    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    entries.sort();
    assert!(
        entries.len() >= 20,
        "expected the full manual under docs/, found {} .md files",
        entries.len()
    );
    for p in entries {
        text.push_str(&fs::read_to_string(&p).unwrap());
        text.push('\n');
    }
    // Labels are matched as phrases; the Markdown wraps lines, so fold all
    // whitespace to single spaces before looking.
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Labels that are not features and need no line in the manual. Keep this
/// short and say why for each.
const ALLOW: &[&str] = &[
    "…",  // the file-picker button beside a path field
    "−",  // the "delete one" button in the dialogs; "+" is under the length rule
    "⇄",  // Compare Runs' swap-A-and-B glyph; the action is documented by its hover text
    "OK", // generic dialog buttons: documented once, collectively, in Appendix B
    "Cancel",
    "Close",
    "Apply",
    "Revert",
    "Add row",
];

#[test]
fn every_swmm_menu_label_and_tool_is_in_the_manual() {
    let docs = docs_text();
    // The menu bar, the map, the dialogs — and every other SWMM pane that
    // hosts buttons, so a pane added later is scanned without editing this.
    let app_src = root().join("app/src");
    let mut sources: Vec<PathBuf> = fs::read_dir(&app_src)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with("swmm_") && n.ends_with(".rs") && !n.contains("test")
        })
        .collect();
    sources.sort();
    assert!(sources.len() >= 20, "expected the SWMM panes under app/src, found {}", sources.len());
    let mut labels: Vec<String> = sources
        .iter()
        .flat_map(|p| labels_in(&fs::read_to_string(p).unwrap()))
        .collect();
    labels.extend(tool_names(&read("app/src/swmm_tools.rs")));
    labels.sort();
    labels.dedup();
    assert!(labels.len() > 60, "label scan found only {} labels", labels.len());

    let missing: Vec<&String> = labels
        .iter()
        .filter(|l| l.chars().count() >= 2)
        .filter(|l| !ALLOW.contains(&l.as_str()))
        // Format strings are runtime text, not labels.
        .filter(|l| !l.contains('{'))
        .filter(|l| !docs.contains(l.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "these SWMM editor labels appear in no docs/*.md file — document the feature \
         (or add the label to ALLOW with a reason):\n  {}",
        missing.iter().map(|s| format!("[{s}]")).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn manual_index_links_every_chapter() {
    let dir = root().join("docs");
    let index = fs::read_to_string(dir.join("index.md")).unwrap();
    let mut unlinked = Vec::new();
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let p = entry.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if p.extension().is_some_and(|x| x == "md") && name != "index.md" && !index.contains(&format!("({name})")) {
            unlinked.push(name);
        }
    }
    assert!(unlinked.is_empty(), "docs/index.md does not link: {unlinked:?}");
}

#[test]
fn manual_cross_links_resolve() {
    let dir = root().join("docs");
    let mut broken = Vec::new();
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        let p = entry.path();
        if !p.extension().is_some_and(|x| x == "md") {
            continue;
        }
        let text = fs::read_to_string(&p).unwrap();
        for target in text.split("](").skip(1) {
            let Some(end) = target.find(')') else { continue };
            let link = &target[..end];
            if link.starts_with("http") || link.starts_with('#') || link.starts_with("mailto:") {
                continue;
            }
            let file = link.split('#').next().unwrap();
            if file.is_empty() {
                continue;
            }
            if !Path::new(&dir).join(file).exists() {
                broken.push(format!("{}: {link}", p.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    assert!(broken.is_empty(), "broken links in docs/: {broken:#?}");
}
