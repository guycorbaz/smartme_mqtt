//! Story 0.7 — crates.io purity: no bridge/application context may leak into the
//! published `sparkplug-b` crate source. Fails if a forbidden token appears anywhere
//! under `src/`.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &["smartme", "ignition", "SMARTME_"];

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

#[test]
fn no_bridge_context_in_crate_source() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    for file in rs_files(&src) {
        let lower = fs::read_to_string(&file).unwrap().to_lowercase();
        for token in FORBIDDEN {
            if lower.contains(&token.to_lowercase()) {
                violations.push(format!("{}: contains '{}'", file.display(), token));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "sparkplug-b source leaks bridge/application context:\n{}",
        violations.join("\n")
    );
}
