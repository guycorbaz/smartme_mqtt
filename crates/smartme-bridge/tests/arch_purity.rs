//! Story 0.6 — the functional-core invariant, enforced mechanically.
//!
//! Pure modules (`core/`, `domain/`) must never import async/transport crates, so no
//! "truth" (a quality/staleness decision) can be taken inside an `async fn`. And the
//! `Measurement` -> Sparkplug metric mapping must live only in
//! `adapters/sparkplug_publisher.rs`. The test scans whatever exists today and becomes
//! active automatically as those modules land — a violation goes red before merge.
//!
//! # What this guard did not see until 2026-08-21
//!
//! It banned four crate names — `tokio`, `rumqttc`, `axum`, `reqwest` — in `core/` and
//! `domain/`. **But the invariant it exists to hold needs no import at all.** `async fn`
//! is a language feature: `pub async fn decide_over_the_network(…)` was added to
//! `core/oracle.rs` — the module whose whole purpose is that no verdict is reached
//! inside a future — and all five tests in this file stayed green.
//!
//! It was the same class of miss as story 8.2's public-API guard, found the same week:
//! a deny-list of the shapes somebody had thought of, blind to the shape the fault
//! ordinarily takes. Both are now allow-lists of what is ours, and the mutations that
//! went past them are kept as fixtures so the guard is re-falsified on every CI pass
//! rather than once by hand.

use std::fs;
use std::path::{Path, PathBuf};

/// Roots a pure module may import from.
///
/// An allow-list of what is ours and the language's, rather than a list of the async
/// crates somebody thought to ban: a dependency added tomorrow is refused here without
/// anyone remembering to add a row.
const PURE_IMPORT_ROOTS: &[&str] = &["crate", "self", "super", "std", "core", "alloc"];

/// Workspace crates a pure module may reach for, and there is exactly one.
///
/// `sparkplug-b` is itself pure — it holds no transport, and `tests/no_context_leak.rs`
/// is what keeps that true — so `domain/quality.rs` re-exporting `sparkplug_b::Quality`
/// is the domain naming the wire's own quality rather than inventing a second one.
/// `smart-me-client` is deliberately absent: it carries `reqwest`, and a pure module
/// reaching for it is exactly the defect this file exists to catch.
const PURE_WORKSPACE_CRATES: &[&str] = &["sparkplug_b"];

/// The one pure module that may name a future, and why it is not a loophole.
///
/// The invariant is **not** that `core/` is synchronous — it is that no *truth* is
/// decided inside a future. `Source` is a port reporting raw facts and deciding
/// nothing, and it is async by native RPITIT, a language feature carrying no runtime.
/// `core/mod.rs` has said exactly this since story 1.4. Everywhere else in `core/` and
/// `domain/`, a decision reached in a future is the defect this file exists to catch.
const ASYNC_PORT_HOME: &str = "core/source.rs";

/// How a decision comes to sit inside a future. No import is needed for any of them.
const ASYNC_TOKENS: &[&str] = &["async fn", "async move", "async {", ".await", "Future"];

/// Every dependency each crate declares, and the claim that goes with the list:
/// **each one has been looked at and cannot move a timestamp into a local zone.**
///
/// `utc_is_the_only_time_domain` used to ban `chrono` and `time` by name. A calendar
/// crate nobody had heard of — `jiff`, `chrono-tz`, `time-tz`, `hifitime` — passed it
/// without a word. Listing what IS declared makes the addition itself the event: the
/// test goes red on any dependency change until somebody adds the row, which is the
/// moment to ask the question.
///
/// `testcontainers` is here as a dev-dependency, and it is precisely the one that
/// drags `chrono` into `Cargo.lock` through `serde_with`'s feature union — recorded in
/// this test's own notes, and harmless because `cargo tree -i chrono` prints nothing.
const DECLARED_DEPENDENCIES: &[(&str, &[&str])] = &[
    (
        "smartme-bridge",
        &[
            "arc-swap",
            "axum",
            "reqwest",
            "rumqttc",
            "serde",
            "serde_json",
            "smart-me-client",
            "sparkplug-b",
            "testcontainers",
            "thiserror",
            "tokio",
            "toml",
            "tracing",
            "tracing-appender",
            "tracing-subscriber",
        ],
    ),
    (
        "smart-me-client",
        &["reqwest", "serde", "serde_json", "thiserror"],
    ),
    ("sparkplug-b", &["prost", "prost-build", "rumqttc", "tokio"]),
];

/// The dependency names one manifest declares, in any of its three dependency tables.
fn declared_dependencies(manifest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_dependencies = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_dependencies = matches!(
                t,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            continue;
        }
        if !in_dependencies || t.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = t.split_once('=') {
            let name = name.trim();
            // `tokio.workspace = true` declares `tokio`, not `tokio.workspace`.
            let name = name.split('.').next().unwrap_or(name).trim();
            if !name.is_empty() && !name.contains(' ') {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

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

/// The crate or module a `use` line reaches into, if the line is one.
fn import_root(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
    let rest = rest.strip_prefix("use ")?;
    Some(
        rest.trim()
            .split("::")
            .next()?
            .trim()
            .trim_start_matches('{')
            .trim()
            .to_string(),
    )
}

/// Judges one pure module. Separate from the walk so the fixture below can be put
/// through the same code the real files go through.
fn purity_violations(relative: &str, text: &str, our_modules: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        // Doc comments SAY what these modules refuse to do; the refusal is checked,
        // not the word.
        if t.starts_with("//") {
            continue;
        }
        if let Some(root) = import_root(t) {
            // A sibling module is named without a `crate::` prefix inside `mod.rs`.
            let ours = PURE_IMPORT_ROOTS.contains(&root.as_str())
                || PURE_WORKSPACE_CRATES.contains(&root.as_str())
                || our_modules.contains(&root);
            if !ours {
                out.push(format!(
                    "{relative}: `{root}` is neither ours nor the language's, so this \
                     module is no longer pure: {t}"
                ));
            }
        }
        if relative != ASYNC_PORT_HOME {
            for token in ASYNC_TOKENS {
                if t.contains(token) {
                    out.push(format!(
                        "{relative}: a verdict may not be reached inside a future — \
                         that is what `core/` and `domain/` are for: {t}"
                    ));
                }
            }
        }
    }
    out
}

#[test]
fn pure_modules_are_free_of_async_and_transport() {
    // The crate's own module names, read rather than listed, so a new module needs no
    // edit here.
    let mut our_modules: Vec<String> = rs_files(&src(""))
        .iter()
        .map(|f| f.file_stem().expect("named").to_string_lossy().into_owned())
        .collect();
    our_modules.sort();
    our_modules.dedup();

    let mut violations = Vec::new();
    let mut scanned = 0;
    for dir in ["core", "domain"] {
        for file in rs_files(&src(dir)) {
            let text = fs::read_to_string(&file).unwrap();
            let relative = format!(
                "{dir}/{}",
                file.file_name().expect("named").to_string_lossy()
            );
            scanned += 1;
            violations.extend(purity_violations(&relative, &text, &our_modules));
        }
    }
    assert!(
        scanned >= 5,
        "only {scanned} pure modules were read; the walk is broken and this test's \
         silence would mean nothing"
    );
    assert!(
        violations.is_empty(),
        "the functional core is no longer pure:\n{}",
        violations.join("\n")
    );
}

/// What went past this guard on 2026-08-21, kept so it cannot go past it again.
///
/// The last two lines matter as much as the first four: a guard that flags everything
/// proves nothing. `use std::fmt` and the crate's own modules must pass.
const IMPURE: &str = r#"
use tokio::time::sleep;
use futures::stream::StreamExt;
use std::fmt;
use crate::domain::Quality;

pub async fn decide(reading: u8) -> u8 {
    fetch().await
}
"#;

#[test]
fn the_purity_scan_still_catches_what_it_was_blind_to() {
    let found = purity_violations("core/oracle.rs", IMPURE, &[]);
    let joined = found.join("\n");

    assert!(
        joined.contains("`tokio`"),
        "an async runtime import went unseen:\n{joined}"
    );
    assert!(
        joined.contains("`futures`"),
        "a crate no deny-list had thought of went unseen — that is the whole point of \
         an allow-list:\n{joined}"
    );
    assert!(
        joined.contains("async fn"),
        "an `async fn` in the deciding module went unseen; it needs no import, which \
         is exactly why the first version of this guard missed it:\n{joined}"
    );
    assert!(
        joined.contains(".await"),
        "an `.await` in the deciding module went unseen:\n{joined}"
    );
    assert!(
        !joined.contains("use std::fmt"),
        "the standard library is not somebody else's crate:\n{joined}"
    );
    assert!(
        !joined.contains("use crate::domain"),
        "a pure module may reach for our own domain:\n{joined}"
    );

    // And the port is exempt by name, not by luck: the same text in `core/source.rs`
    // is judged only on its imports.
    let at_the_port = purity_violations(ASYNC_PORT_HOME, IMPURE, &[]).join("\n");
    assert!(
        !at_the_port.contains("async fn"),
        "the async port must keep its future; it decides nothing:\n{at_the_port}"
    );
    assert!(
        at_the_port.contains("`tokio`"),
        "the port is exempt from the future, not from purity:\n{at_the_port}"
    );
}

/// Stories 1.3/1.4: raw time sources and the test doubles that fabricate inputs are
/// each confined to their home file. Everywhere else — including inline
/// `#[cfg(test)]` modules — time arrives through the injected `Clock` and readings
/// through the injected `Source`, or a staleness decision could silently depend on
/// a hardcoded `now()` / a fake wired into production.
#[test]
fn raw_time_sources_and_fakes_are_confined_to_their_home_modules() {
    // (banned token, the single file allowed to contain it). Raw time sources
    // are banned EVERYWHERE, inline test modules included: a non-deterministic
    // test of deterministic code is a contradiction. The test doubles are banned
    // only in PRODUCTION code — a fake wired into the app is the hazard; a fake
    // used by a test is the point — so their scan stops at the file's
    // `#[cfg(test)]` marker.
    const CONFINED_TOKENS: &[(&str, &str, bool)] = &[
        ("Instant::now(", "core/clock.rs", true),
        ("SystemTime::now(", "core/clock.rs", true),
        ("use std::time::Instant", "core/clock.rs", true),
        ("use std::time::SystemTime", "core/clock.rs", true),
        // Added 2026-08-21 (issue #106). The four tokens above name the two
        // constructors; these two name the way a raw clock is read WITHOUT
        // constructing anything — `UNIX_EPOCH.elapsed()` is a wall-clock read that
        // mentions neither `SystemTime::now` nor `Instant::now`, and `.elapsed()` on
        // a stored instant is a duration nobody injected. Both live in
        // `core/clock.rs` today and nowhere else, which is what makes the rule free.
        ("UNIX_EPOCH", "core/clock.rs", true),
        (".elapsed(", "core/clock.rs", true),
        ("FakeClock", "core/clock.rs", false),
        ("FakeSource", "core/source.rs", false),
        ("poll_now(", "core/source.rs", false),
    ];
    // Story 1.11: the state machine lives ENTIRELY in the poll task. The mqtt
    // task knows only connection birth and death, so it may not even NAME the
    // machine; and nobody outside the poll task may RUN it (the supervisor is
    // the composition root and legitimately passes the policy through as
    // configuration, so it is judged on `.step(`, not on the type name).
    const NAMING_BANNED_IN_MQTT: &[&str] = &["state_machine", "Policy", "State::"];
    const RUNNING_BANNED_OUTSIDE_POLL: &str = ".step(";
    let mut violations = Vec::new();
    for file in rs_files(&src("")) {
        let text = fs::read_to_string(&file).unwrap();
        let mut in_test_module = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("#[cfg(test)]") {
                in_test_module = true;
            }
            if t.starts_with("//") {
                continue;
            }
            for (banned, home, applies_in_tests) in CONFINED_TOKENS {
                if file.ends_with(home) || (in_test_module && !applies_in_tests) {
                    continue;
                }
                if t.contains(banned) {
                    violations.push(format!("{}: {}", file.display(), t));
                }
            }
        }
    }
    for file in rs_files(&src("app")) {
        if file.ends_with("poll_publish.rs") {
            continue;
        }
        let is_mqtt_task = file.ends_with("mqtt_driver.rs");
        let text = fs::read_to_string(&file).unwrap();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("//") {
                continue;
            }
            if t.contains(RUNNING_BANNED_OUTSIDE_POLL) {
                violations.push(format!(
                    "{}: only the poll task may run the state machine: {}",
                    file.display(),
                    t
                ));
            }
            if is_mqtt_task {
                for banned in NAMING_BANNED_IN_MQTT {
                    if t.contains(banned) {
                        violations.push(format!(
                            "{}: the mqtt task must not reach for a verdict: {}",
                            file.display(),
                            t
                        ));
                    }
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "raw time sources / test fakes outside their home modules:\n{}",
        violations.join("\n")
    );
}

/// Story 2.7 AC3 — UTC is the only time domain, asserted mechanically.
///
/// FR10 asks that all timestamps be treated as UTC end-to-end. The two parsers
/// enforce it at the boundary — each refuses a timestamp that does not
/// explicitly declare UTC, and their own tests say why. This test enforces the
/// rest of the path: no crate of ours may declare a calendar/timezone library or
/// name a zoned time type, so there is no code that COULD move a timestamp into
/// a local zone between the parse and the wire. `UtcMillis` stays the only time
/// type, and the raw wall clock stays confined to `core/clock.rs` (the test
/// above this one).
///
/// The scan covers all three crates, not only this one: the timestamps are born
/// in `smart-me-client` and reach the wire through `sparkplug-b`.
///
/// One finding recorded rather than asserted: `Cargo.lock` DOES list `chrono`.
/// It arrives through `testcontainers` (a dev-dependency of this crate) via
/// `serde_with`'s feature union, and `cargo tree -i chrono` prints nothing — the
/// lockfile is the union of everything that COULD be built for any target or
/// feature set, not what is. What we control is what our manifests request and
/// what our sources name, and that is what this test pins.
#[test]
fn utc_is_the_only_time_domain() {
    // Distinctive tokens only — `chrono` as a bare substring would flag the word
    // "synchronous", so the crate is matched the way it must be written to be
    // used. Comment lines are skipped, as everywhere in this file.
    const BANNED_TOKENS: &[&str] = &[
        "use chrono",     // the calendar crate, however aliased afterwards
        "chrono::",       // or path-qualified without a use
        "OffsetDateTime", // the `time` crate's zoned type
        "PrimitiveDateTime",
        "FixedOffset",   // chrono's offset type
        "with_timezone", // chrono's zone conversion
        "Local::now",    // a local-zone "now" from any crate
        "localtime",
    ];
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ exists")
        .to_path_buf();
    let mut violations = Vec::new();
    for krate in ["smartme-bridge", "smart-me-client", "sparkplug-b"] {
        for file in rs_files(&workspace.join(krate).join("src")) {
            let text = fs::read_to_string(&file).unwrap();
            for line in text.lines() {
                let t = line.trim();
                if t.starts_with("//") {
                    continue;
                }
                for banned in BANNED_TOKENS {
                    if t.contains(banned) {
                        violations.push(format!("{}: {}", file.display(), t));
                    }
                }
            }
        }
        // And the manifest: a dependency nobody imports yet is still a door.
        //
        // **An allow-list since 2026-08-21 (issue #106).** It used to name `chrono`
        // and `time` — the two doors somebody had thought of — so `jiff`, `chrono-tz`,
        // `time-tz` or `hifitime` would all have walked straight past it. Naming the
        // dependencies that ARE declared instead turns "did anyone remember to ban
        // it?" into "was this one looked at?", which is a question a diff answers.
        let declared = declared_dependencies(
            &fs::read_to_string(workspace.join(krate).join("Cargo.toml")).unwrap(),
        );
        let allowed: &[&str] = DECLARED_DEPENDENCIES
            .iter()
            .find(|(name, _)| *name == krate)
            .map(|(_, list)| *list)
            .expect("every crate of the workspace is listed");
        for name in &declared {
            if !allowed.contains(&name.as_str()) {
                violations.push(format!(
                    "{krate}/Cargo.toml declares `{name}`, which nobody has looked at \
                     for a time-zone door. If it cannot move a timestamp into a local \
                     zone, add it to DECLARED_DEPENDENCIES and say so there."
                ));
            }
        }
        for name in allowed {
            if !declared.iter().any(|d| d == name) {
                violations.push(format!(
                    "{krate}/Cargo.toml no longer declares `{name}`; the list in this \
                     test is stale, and a stale allow-list is how one stops being read."
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "a local-time capability reached the workspace; every timestamp here is \
         UTC epoch-millis and a zoned type is how one silently stops being:\n{}",
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

/// **Story 6.6 AC3 — the end-to-end check publishes nothing, enforced mechanically.**
///
/// The criterion says *"no MQTT message of any kind is produced by the check"*, and
/// the honest way to hold that is structurally: the module has no handle that could
/// produce one, and this test makes acquiring one visible. A runtime assertion could
/// only observe the messages a particular test path did not send.
///
/// **Why it is worth a guard rather than a comment.** The obvious way to prove a
/// sink works is to publish to it, and the next person to touch this screen will
/// think of it. A DDATA carrying a test value is, in the historian, indistinguishable
/// from a measurement — the button would manufacture the exact lie this bridge
/// exists to refuse — so the third link is the driver's OBSERVED state (story 6.5)
/// and must stay that.
///
/// FALSIFIED 2026-08-20 — mutation RUN, output copied: adding
/// `use crate::app::poll_publish::Publication;` to `ui/check.rs` goes red with
///
/// ```text
/// thread 'the_end_to_end_check_cannot_publish' panicked at
/// crates/smartme-bridge/tests/arch_purity.rs:288:5:
/// the end-to-end check must not be able to publish (story 6.6 AC3): the third link
/// is the driver's observed state, and a test value on the wire is indistinguishable
/// from a measurement in the historian:
/// …/src/ui/check.rs: use crate::app::poll_publish::Publication;
/// ```
/// What `ui/check.rs` may reach for, as whole path prefixes.
///
/// **Added 2026-08-21 (issue #106).** The word list below it was blind to the shape
/// this fault ordinarily takes: nobody writes `Publication` into a UI handler, they
/// reach a handle through the state the handler already holds. `UiState` carries an
/// `Arc<tokio::sync::Notify>` today — a nudge, deliberately — and the day it carries a
/// sender called `tx` or `sink`, not one banned word appears anywhere.
///
/// So the module's imports are an allow-list instead. `crate::app::` is where the
/// publisher lives and is absent from this list on purpose; `smart_me_client` is
/// present because fetching from the meter is the whole point of the second link.
const CHECK_MAY_IMPORT: &[&str] = &[
    "super::",
    "std::",
    "core::",
    "axum::",
    "smart_me_client::",
    "crate::core::",
    "crate::domain::",
];

#[test]
fn the_end_to_end_check_cannot_publish() {
    let file = src("ui/check.rs");
    let text = fs::read_to_string(&file).expect("story 6.6 ships this module");
    // `.send(` joins the word list: a channel send is how a handle acquired through
    // the shared state is actually used, whatever the field is called.
    let banned = [
        "Outbox",
        "outbox",
        "Publisher",
        "publish(",
        "Publication",
        ".send(",
    ];
    let mut violations = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        // Doc comments SAY what this module refuses to do; the refusal is what is
        // checked, not the word.
        if t.starts_with("//") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("use ") {
            let path = rest.trim();
            if !CHECK_MAY_IMPORT.iter().any(|ok| path.starts_with(ok)) {
                violations.push(format!(
                    "{}: reaches outside what the check may hold: {}",
                    file.display(),
                    t
                ));
            }
        }
        for word in banned {
            if t.contains(word) {
                violations.push(format!("{}: {}", file.display(), t));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "the end-to-end check must not be able to publish (story 6.6 AC3): the third \
         link is the driver's observed state, and a test value on the wire is \
         indistinguishable from a measurement in the historian:\n{}",
        violations.join("\n")
    );
}
