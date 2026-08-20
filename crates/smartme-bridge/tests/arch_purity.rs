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
        let manifest = fs::read_to_string(workspace.join(krate).join("Cargo.toml")).unwrap();
        for line in manifest.lines() {
            let t = line.trim();
            if t.starts_with("chrono") || t.starts_with("time =") || t.starts_with("time.") {
                violations.push(format!("{krate}/Cargo.toml declares a time-zone door: {t}"));
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
#[test]
fn the_end_to_end_check_cannot_publish() {
    let file = src("ui/check.rs");
    let text = fs::read_to_string(&file).expect("story 6.6 ships this module");
    let banned = ["Outbox", "outbox", "Publisher", "publish(", "Publication"];
    let mut violations = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        // Doc comments SAY what this module refuses to do; the refusal is what is
        // checked, not the word.
        if t.starts_with("//") {
            continue;
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
