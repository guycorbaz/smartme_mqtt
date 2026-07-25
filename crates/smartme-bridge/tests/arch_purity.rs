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
