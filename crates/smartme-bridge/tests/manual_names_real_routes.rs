//! Story 8.1 AC4 — the manual may not name a route this bridge does not serve.
//!
//! **A manual is the one artefact whose defects no compiler catches.** Every other
//! claim in this repository is checked by something: a type, a test, a gate. A
//! sentence that sends an operator to `/status` — a page that never existed — is
//! found by the operator, at three in the morning, during the incident that made
//! them open the manual.
//!
//! This is deliberately narrow. It does not check that the manual is TRUE; it
//! checks the one class of claim that can be checked mechanically and that goes
//! stale silently: the routes. Routes move — story 6.6 added `/check`, and nothing
//! would have noticed the day one was renamed.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<crate>/..")
        .to_path_buf()
}

/// Every path `ui::routes` registers, read from the source.
///
/// **Read rather than listed**, so a renamed route changes this set without anybody
/// remembering to. A hand-kept list would be a third place the truth lives, and the
/// one most likely to be forgotten.
fn routes_served() -> BTreeSet<String> {
    let source = fs::read_to_string(repo().join("crates/smartme-bridge/src/ui/mod.rs"))
        .expect("the UI module");
    // **Scanned across newlines, not line by line.** The first version read one line
    // at a time and missed every route whose path sits on the line BELOW `.route(`
    // — which is how `axum` routes get formatted once they carry two handlers. It
    // reported `/config` and `/confirm` as routes the manual had invented. The
    // length guard below is what caught it: the scan is checked before it is
    // trusted.
    let mut found = BTreeSet::new();
    for chunk in source.split(".route(").skip(1) {
        let Some(start) = chunk.find('"') else {
            continue;
        };
        let after = &chunk[start + 1..];
        let Some(end) = after.find('"') else { continue };
        let path = &after[..end];
        if path.starts_with('/') {
            found.insert(path.to_string());
        }
    }
    found
}

/// Paths the manual names that are NOT HTTP routes, and why each is here.
///
/// A short list, and every entry is a path in a filesystem rather than on this
/// bridge. Anything not named here is checked.
const NOT_ROUTES: &[&str] = &[
    // The state directory inside the container, and the smoke test's mount.
    "/data",
    "/state",
    // smart-me's own API, quoted when describing what the bridge asks of it.
    "/oauth/claimtree",
    "/actions",
];

/// Is this a path on THIS bridge at all?
///
/// A LaTeX-escaped placeholder — `/Devices/\{id\}` — is smart-me's API being
/// quoted, not a route here.
fn looks_like_our_route(candidate: &str) -> bool {
    candidate.starts_with('/')
        && candidate.len() > 1
        && !candidate.contains('.')
        && !candidate.contains('\\')
        && !candidate.contains('{')
}

#[test]
fn every_route_the_manual_names_is_one_the_bridge_serves() {
    let served = routes_served();
    assert!(
        served.contains("/healthz") && served.len() >= 5,
        "the route scan found {served:?}, which is too few to be the real set — the \
         scan is broken and this test would then prove nothing"
    );

    let chapters = repo().join("docs/manual/chapters");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&chapters).expect("the manual's chapters") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "tex") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("chapter");
        // `\code{/meters}` and `\code{GET /healthz}` are the two shapes the manual
        // uses. Anything with a dot in it is a filename, not a route.
        for chunk in text.split("\\code{").skip(1) {
            let Some(end) = chunk.find('}') else { continue };
            let body = &chunk[..end];
            let candidate = body.rsplit(' ').next().unwrap_or(body);
            if !looks_like_our_route(candidate) {
                continue;
            }
            if NOT_ROUTES.contains(&candidate) || served.contains(candidate) {
                continue;
            }
            offenders.push(format!(
                "{}: names {candidate}",
                path.file_name().expect("named").to_string_lossy()
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "the manual names routes this bridge does not serve. An operator following \
         one of these during an incident finds a 404, and the manual is the one \
         artefact whose defects no compiler catches:\n{}\n\nServed today: {served:?}",
        offenders.join("\n")
    );
}
