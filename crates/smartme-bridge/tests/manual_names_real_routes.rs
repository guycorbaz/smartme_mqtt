//! Story 8.1 AC4 — the manual may not name a route this bridge does not serve.
//!
//! **A manual is the one artefact whose defects no compiler catches.** Every other
//! claim in this repository is checked by something: a type, a test, a gate. A
//! sentence that sends an operator to `/status` — a page that never existed — is
//! found by the operator, at three in the morning, during the incident that made
//! them open the manual.
//!
//! This is deliberately narrow. It does not check that the manual is TRUE; it
//! checks the two classes of claim that can be checked mechanically and that go
//! stale silently: **the routes it sends an operator to**, and **the causes it
//! names**. Both move — story 6.6 added `/check`, story 6.8 gave every cause a
//! gesture — and nothing would notice the day one was renamed.

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

/// **Story 8.1, review — the manual may not name a cause the oracles cannot produce.**
///
/// The troubleshooting chapter is written around cause slugs: `credential-rejected`,
/// `feed-not-advancing`, `source-rate-limited`. Those are the words that appear on
/// the wire and on the screens, and they are what an operator matches against what
/// they are seeing. **A slug that no longer exists sends them looking for a string
/// the bridge never prints** — and unlike a route, nothing 404s to tell them.
///
/// The cause vocabulary is part of the published contract (chapter 5), so it changes
/// only with a `CONTRACT_VERSION` bump — which makes this check cheap and its
/// failures meaningful.
///
/// FALSIFIED 2026-08-20 — mutation RUN, output copied: renaming a cause in the
/// manual to `credential-refused` goes red with
///
/// ```text
/// the manual names causes the oracles cannot produce … 07-troubleshooting.tex:
/// names credential-refused
/// ```
#[test]
fn every_cause_the_manual_names_is_one_the_oracles_can_produce() {
    // **BOTH vocabularies, because the manual quotes both.** `Cause` is what the
    // oracles decide about a reading; `DropReason` is what the bridge lost and why.
    // They are different enums and they appear side by side on `/healthz` and on the
    // screens — a test that knew only the first would have called five real slugs
    // inventions, which is what its first run did.
    let live: BTreeSet<&str> = smartme_bridge::core::oracle::Cause::ALL
        .iter()
        .map(|c| c.as_str())
        .chain(
            smartme_bridge::app::poll_publish::DropReason::ALL
                .iter()
                .map(|r| r.as_str()),
        )
        .collect();
    assert!(
        live.len() >= 26 && live.contains("credential-rejected") && live.contains("outbox-full"),
        "the cause vocabulary read {live:?}, which is not the real one — this test \
         would then prove nothing"
    );

    // Slugs are lower-case words joined by hyphens, and the manual writes them in
    // `\code{}` like everything else it quotes verbatim.
    let mut offenders = Vec::new();
    for entry in fs::read_dir(repo().join("docs/manual/chapters")).expect("chapters") {
        let path = entry.expect("entry").path();
        if path.extension().is_none_or(|e| e != "tex") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("chapter");
        for chunk in text.split("\\code{").skip(1) {
            let Some(end) = chunk.find('}') else { continue };
            let body = &chunk[..end];
            // WHAT IS NOT A CAUSE, and each exclusion is a family rather than a
            // name: the specification's own clause identifiers (`tck-id-…`, and the
            // `-dbirth-retain` fragments a list of them elides to), and this
            // workspace's crate names. Everything else that has the shape of a slug
            // is checked — a cause misspelled as `credential-refused` is exactly
            // what this exists to catch.
            let is_clause_id = body.starts_with("tck-id-") || body.starts_with('-');
            let is_crate_name = matches!(
                body,
                "sparkplug-b" | "smart-me-client" | "smartme-bridge" | "cargo-deny"
            );
            let looks_like_a_slug = body.contains('-')
                && body.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && body.len() > 6
                && !is_clause_id
                && !is_crate_name;
            if looks_like_a_slug && !live.contains(body) {
                offenders.push(format!(
                    "{}: names {body}",
                    path.file_name().expect("named").to_string_lossy()
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "the manual names causes the oracles cannot produce. An operator matching \
         what they see against these finds nothing, and unlike a route there is no \
         404 to tell them:\n{}\n\nLive vocabulary: {live:?}",
        offenders.join("\n")
    );
}
