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

/// Every route-shaped word in a chapter, `\code{}` or not.
///
/// **Added 2026-08-21 (issue #106).** The scan below read only the bodies of
/// `\code{…}`, which is the manual's convention — and a convention is not a
/// mechanism. `06-operations-ui.tex` already names `/healthz` in running prose, so
/// the ordinary way this guard goes blind is not exotic: somebody writes "ouvrez
/// /status" in a sentence and no macro is involved.
///
/// **What makes this cheap is a shape narrow enough to need no exception list.** A
/// candidate is a SINGLE lower-case segment introduced by a slash that nothing
/// alphanumeric precedes. That rules out, without naming anything: `fresh/stale` and
/// `and/or` (a letter before the slash), `https://api.smart-me.com/actions` and
/// `docs/adr/0009-….md` (a slash or a letter before it), `spBv1.0/Site/NDATA/Bridge`
/// (upper case), `\code{seq}/\code{bdSeq}/rebirth` (a closing brace before it), and
/// `/data/logs` (a second segment). Over the manual as it stands it yields exactly the
/// routes the `\code{}` scan already found, and nothing else.
///
/// **What it therefore cannot see, said plainly**: the two multi-segment routes,
/// `/config/discover` and `/debug/panic`. A sentence inventing `/config/refresh` would
/// go unnoticed here. Widening the shape to reach them means readmitting every
/// filesystem path and every Sparkplug topic in the manual, and paying for it with the
/// exception list this scan exists to avoid — so the narrow shape is the deliberate
/// choice, and this paragraph is the record of what it costs.
fn prose_route_candidates(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '/' {
            i += 1;
            continue;
        }
        let before = if i == 0 { ' ' } else { chars[i - 1] };
        // A slash inside a word, a path, a URL or a topic — not the start of a route.
        // `}` too: `\code{seq}/\code{bdSeq}/rebirth` is an enumeration written with
        // macros, which is how the first run of this scan read `/rebirth` as a route.
        if before.is_alphanumeric()
            || before == '/'
            || before == '.'
            || before == '\\'
            || before == '}'
        {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < chars.len() && chars[end].is_ascii_lowercase() {
            end += 1;
        }
        if end > start {
            let after = chars.get(end).copied();
            let ends_cleanly = match after {
                // End of file.
                None => true,
                // A second segment, a longer word, a file extension: not our route.
                Some(c) if c.is_alphanumeric() || c == '/' || c == '-' || c == '_' => false,
                // A full stop ENDS A SENTENCE unless a word follows it, in which case
                // it is `config.toml` rather than "…ouvrez /config."
                Some('.') => !chars
                    .get(end + 1)
                    .copied()
                    .is_some_and(|c| c.is_alphanumeric()),
                Some(_) => true,
            };
            if ends_cleanly {
                out.push(format!("/{}", chars[start..end].iter().collect::<String>()));
            }
        }
        i = end.max(i + 1);
    }
    out
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
        // And the same question of the prose, where no macro marks the claim.
        for candidate in prose_route_candidates(&text) {
            if NOT_ROUTES.contains(&candidate.as_str()) || served.contains(&candidate) {
                continue;
            }
            offenders.push(format!(
                "{}: names {candidate} in prose",
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
    //
    // **And the neutral value, which is a slug and belongs to neither enum.** From
    // contract v11 a good metric publishes `Cause = no-cause` (ADR 0043), so the
    // manual names a string the oracles cannot produce and never will. It is read
    // from the constant the publisher actually sends rather than written out here:
    // a vocabulary this test spells for itself is a vocabulary that drifts from the
    // wire in silence, which is the whole defect this test exists to prevent.
    let live: BTreeSet<&str> = smartme_bridge::core::oracle::Cause::ALL
        .iter()
        .map(|c| c.as_str())
        .chain(
            smartme_bridge::app::poll_publish::DropReason::ALL
                .iter()
                .map(|r| r.as_str()),
        )
        .chain(std::iter::once(
            smartme_bridge::adapters::sparkplug_publisher::CAUSE_NONE,
        ))
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

/// What the prose scan must see, and — as much of the point — what it must not.
///
/// Kept as a fixture rather than a mutation performed once: a scanner this narrow is
/// only worth having if it still discriminates after the next edit to it.
const PROSE: &str = r"
Ouvrez /status pour voir l'état, puis \code{/healthz} pour la sonde.
Une lecture est fresh/stale, et l'opérateur lit and/or selon le cas.
Le journal vit dans /data/logs et l'API est https://api.smart-me.com/actions.
Le sujet est spBv1.0/Site/NDATA/Bridge01, et \code{seq}/\code{bdSeq}/rebirth.
Le fichier s'appelle config.toml et se trouve sous /data.
";

#[test]
fn the_prose_scan_sees_a_route_in_a_sentence_and_nothing_else() {
    let found = prose_route_candidates(PROSE);

    for expected in ["/status", "/healthz", "/data"] {
        assert!(
            found.iter().any(|c| c == expected),
            "the prose scan missed {expected}: {found:?}"
        );
    }
    // `/status` is the whole reason this scan exists: a route named in a sentence,
    // with no macro to mark it.
    for noise in [
        "/stale",   // fresh/stale
        "/or",      // and/or
        "/logs",    // a second segment
        "/actions", // inside a URL
        "/rebirth", // an enumeration of \code{} macros
        "/toml",    // a file extension
        "/site",    // a Sparkplug topic level
    ] {
        assert!(
            !found.iter().any(|c| c == noise),
            "the prose scan cried wolf over {noise}, and a guard that flags everything \
             is worth as little as one that flags nothing: {found:?}"
        );
    }
}
