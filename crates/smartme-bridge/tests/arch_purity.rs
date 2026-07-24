//! Story 0.6 — the functional-core invariant, enforced mechanically.
//!
//! Pure modules (`core/`, `domain/`) must never import async/transport crates, so no
//! "truth" (a quality/staleness decision) can be taken inside an `async fn`. And the
//! `Measurement` -> Sparkplug metric mapping must live only in
//! `adapters/sparkplug_publisher.rs`. The test scans whatever exists today and becomes
//! active automatically as those modules land — a violation goes red before merge.

use std::fs;
use std::path::{Path, PathBuf};

const BANNED_IN_PURE: &[&str] = &["tokio", "rumqttc", "axum", "reqwest"];

fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out
}

fn src(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(rel)
}

#[test]
fn pure_modules_are_free_of_async_and_transport() {
    let mut violations = Vec::new();
    for dir in ["core", "domain"] {
        for file in rs_files(&src(dir)) {
            let text = fs::read_to_string(&file).unwrap();
            for line in text.lines() {
                let t = line.trim();
                if t.starts_with("//") {
                    continue;
                }
                for banned in BANNED_IN_PURE {
                    if t.contains(&format!("use {banned}")) || t.contains(&format!("{banned}::")) {
                        violations.push(format!("{}: {}", file.display(), t));
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "pure modules import banned async/transport crates:\n{}",
        violations.join("\n")
    );
}

#[test]
fn measurement_to_sparkplug_mapping_is_confined_to_the_publisher() {
    let mut violations = Vec::new();
    for file in rs_files(&src("adapters")) {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if name == "sparkplug_publisher.rs" {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap();
        // An adapter that both names `Measurement` and builds Sparkplug protobuf metrics
        // is doing the mapping — that must live only in the publisher.
        if text.contains("Measurement")
            && (text.contains("protobuf::") || text.contains("sparkplug_b::"))
        {
            violations.push(file.display().to_string());
        }
    }
    assert!(
        violations.is_empty(),
        "Measurement->Sparkplug mapping must live only in adapters/sparkplug_publisher.rs; found in:\n{}",
        violations.join("\n")
    );
}
