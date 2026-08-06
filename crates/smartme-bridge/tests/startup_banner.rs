//! The startup banner must survive the failure it exists to explain.
//!
//! # Why this is a test and not a glance at the code
//!
//! A version line is trivially easy to write and trivially easy to place where
//! it does no good. The case an operator needs it in is the container that is
//! **up but doing nothing**: one whose configuration is wrong, which therefore
//! never reaches the publishing path, and whose entire output would otherwise be
//!
//! ```text
//! Error: the configuration was refused; 2 problem(s) found …
//! ```
//!
//! A banner emitted after the configuration is read would be absent from exactly
//! that log. So the property under test is not *"the version is logged"* — it is
//! **"the version is logged even when the configuration is refused"**, and only
//! running the real binary can show that.
//!
//! Requested by Guy on 2026-08-01 after `v0.3.0` went into service, when the
//! recurring question during that day's work had been *"is this the published
//! image or the working tree?"* — a question the image tag cannot answer, since
//! the tag and the compiled version can drift. `CARGO_PKG_VERSION` is resolved at
//! compile time and cannot.
//!
//! # These tests must never wait for the process to end
//!
//! **Until [ADR 0026] a refused configuration exited, and this file called
//! `Command::output()`, which waits for exactly that.** The ADR keeps the process
//! up so the screen that repairs the configuration is reachable — and had the
//! helper been left alone, all three tests would have hung for ever rather than
//! failed. That is the failure mode this repository has already paid for twice:
//! a binary-spawning check does not go red when its premise dies, it goes quiet.
//!
//! So the helper below spawns, reads both pipes until the markers it needs appear
//! or a deadline passes, and then kills. It never asks whether the child exited,
//! because the answer is meant to be "no".
//!
//! [ADR 0026]: ../../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Removes ANSI colour sequences.
///
/// `tracing`'s console layer writes them BETWEEN a field's name and its `=`, so
/// a naive `contains("contract=3")` fails against output that plainly reads
/// `contract=3` on screen. Found by this test failing while the feature worked —
/// the same class of mistake as masking secrets with a pattern written for the
/// wrong output format. The file sink sets `.with_ansi(false)` for this reason;
/// the console one cannot, because colour is the point there.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip through the terminating letter of the escape sequence.
            for e in chars.by_ref() {
                if e.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A scratch state directory, empty.
fn state_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("smartme_banner_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

/// How long to let the bridge talk before giving up. Generous: the cost of a
/// slow CI runner is a slow test, and the cost of being stingy is a flake in the
/// one file whose whole subject is what a struggling deployment prints.
const DEADLINE: Duration = Duration::from_secs(20);

/// Runs the bridge with a configuration that EXISTS, parses, and is refused by
/// validation, and returns everything it said before being stopped.
///
/// **The distinction is the subject of the test, since 2026-08-04.** Before
/// [ADR 0023] this helper withheld environment variables, and an incomplete
/// configuration meant a refusal to start. It no longer does: a bridge with *no*
/// configuration comes up and serves the UI. The identity is therefore written to
/// the file and left **empty** — present, readable, and invalid, which is the
/// case this file is about.
///
/// `ui_port` is per-case because all three tests run concurrently and the bridge
/// now serves the UI in this state ([ADR 0026]); sharing the default would leave
/// two of the three logging a bind failure that has nothing to do with them.
///
/// [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
/// [ADR 0026]: ../../../docs/adr/0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md
fn run_with_missing_identity(case: &str, ui_port: u16, want: &[&str]) -> (String, String) {
    // A directory PER CASE. The three tests below run concurrently and this
    // helper deletes the directory on the way out, so a shared name means one
    // test removing the configuration another has not finished reading —
    // a flake that showed up the first time all three ran together.
    let dir = state_dir(case);
    std::fs::write(
        dir.join("config.toml"),
        // Parses, matches the schema, and is refused by validation — which is a
        // different code path from a file that does not parse at all.
        format!(
            // Read from the constant, never spelled out: written as a literal `2`
            // this rotted the moment story 5.3 bumped the schema, and the test then
            // failed for a schema fault instead of the identity fault it is about —
            // right colour, wrong reason.
            "schema_version = {}\n\
         group_id = \"\"\n\
         node_id = \"\"\n\
         broker_host = \"192.0.2.1\"\n\
         broker_port = 1883\n\
         publish_period_secs = 30\n\
         ui_port = {ui_port}\n\
         # AT THE ROOT, before the meters table. Appended after it, TOML makes it\n\
         # a member of the last [[meters]] — which is the exact trap recorded in\n\
         # store.rs's unknown-field test, walked into again here.\n\
         mapping_confirmed = true\n\
         \n\
         [[meters]]\n\
         meter_id = \"m\"\n\
         device_id = \"d\"\n\
         serial = \"9202685\"\n\
         enabled = true\n",
            smartme_bridge::app::store::SCHEMA_VERSION
        ),
    )
    .expect("write config");

    let mut child = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", &dir)
        .env("SMARTME_CLIENT_ID", "x")
        .env("SMARTME_CLIENT_SECRET", "x")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the bridge binary runs");

    // Both pipes are drained by their own thread. A single thread reading one
    // then the other deadlocks the moment the unread pipe fills, and the whole
    // point here is a process that keeps talking.
    let out = Arc::new(Mutex::new(String::new()));
    let err = Arc::new(Mutex::new(String::new()));
    let mut readers = Vec::new();
    for (pipe, sink) in [
        (
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            Arc::clone(&out),
        ),
        (
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            Arc::clone(&err),
        ),
    ] {
        let pipe = pipe.expect("a piped stream");
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                let mut held = sink.lock().unwrap_or_else(|e| e.into_inner());
                held.push_str(&line);
                held.push('\n');
            }
        }));
    }

    // Stop as soon as everything asked for has been seen, so a healthy run is
    // fast and only a broken one pays the deadline.
    let started = Instant::now();
    loop {
        let seen = {
            let (o, e) = (
                out.lock().unwrap_or_else(|p| p.into_inner()),
                err.lock().unwrap_or_else(|p| p.into_inner()),
            );
            let all = strip_ansi(&format!("{o}{e}"));
            want.iter().all(|needle| all.contains(needle))
        };
        if seen || started.elapsed() > DEADLINE {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Killing closes the pipes, which ends the reader threads.
    let _ = child.kill();
    let _ = child.wait();
    for reader in readers {
        let _ = reader.join();
    }

    let stdout = strip_ansi(&out.lock().unwrap_or_else(|p| p.into_inner()).clone());
    let stderr = strip_ansi(&err.lock().unwrap_or_else(|p| p.into_inner()).clone());
    let _ = std::fs::remove_dir_all(&dir);
    (stdout, stderr)
}

/// The premise every test here rests on: the configuration really was refused.
///
/// It used to be `!status.success()`. Since [ADR 0026] the process does not exit,
/// so the observable is the fault itself — and it must stay checked rather than
/// assumed, or these tests would be asserting the banner on a HEALTHY start,
/// which is the easy case and not the one that matters.
fn assert_the_configuration_was_refused(all: &str) {
    assert!(
        all.contains("config.toml: group_id"),
        "the identity guard did not name the key the operator edits, so this run \
         is not the refusal these tests are about:\n{all}"
    );
    assert!(
        all.contains("NOTHING is published"),
        "the bridge did not report withholding everything; since ADR 0026 that \
         line is what distinguishes a refused configuration from a working one, \
         now that the process no longer exits to say so:\n{all}"
    );
}

#[test]
fn the_version_is_logged_even_when_the_configuration_is_refused() {
    let (stdout, stderr) = run_with_missing_identity(
        "version",
        18181,
        &["config.toml: group_id", "smartme_mqtt starting"],
    );
    let all = format!("{stdout}{stderr}");

    assert_the_configuration_was_refused(&all);
    assert!(
        all.contains(env!("CARGO_PKG_VERSION")),
        "the package version {} does not appear anywhere in the output of a bridge whose \
         configuration was refused. That is the log an operator reads when a container is \
         up and publishing nothing, and it is the one case where knowing the build matters \
         most:\n{all}",
        env!("CARGO_PKG_VERSION")
    );
    assert!(
        all.contains("smartme_mqtt starting"),
        "the startup banner is missing:\n{all}"
    );
}

#[test]
fn the_banner_precedes_the_failure() {
    let (stdout, stderr) = run_with_missing_identity(
        "order",
        18182,
        &["config.toml: group_id", "smartme_mqtt starting"],
    );

    // ORDER, not mere presence. A banner that appears after the error would still
    // satisfy the test above if both landed in the same capture, and would still
    // be useless in a log read top-down during an incident.
    //
    // The two go to different streams — the banner is traced to stdout, the fault
    // to stderr — so their relative order cannot be read from a single
    // concatenation. What IS provable, and what actually matters, is that the
    // banner exists on stdout, which is written before the configuration is read.
    assert_the_configuration_was_refused(&format!("{stdout}{stderr}"));
    assert!(
        stdout.contains("smartme_mqtt starting"),
        "the banner must be on stdout, emitted before the configuration was read; \
         stdout was:\n{stdout}"
    );
    assert!(
        stderr.contains("config.toml: group_id"),
        "the failure must still reach stderr — since ADR 0026 the process stays up, \
         and `docker compose logs` is where an operator without a browser looks; \
         stderr was:\n{stderr}"
    );
}

#[test]
fn the_banner_carries_the_contract_version() {
    let contract = smartme_bridge::adapters::sparkplug_publisher::CONTRACT_VERSION;
    let (stdout, stderr) = run_with_missing_identity(
        "contract",
        18183,
        &["config.toml: group_id", &format!("contract={contract}")],
    );
    let all = format!("{stdout}{stderr}");

    assert_the_configuration_was_refused(&all);
    // The contract version is what a CONSUMER sees. It answers a different
    // question from the package version — "what will this put on the wire?" —
    // and it is the first thing worth knowing when a tag looks wrong in Ignition.
    assert!(
        all.contains(&format!("contract={contract}")),
        "the banner must state the contract version ({contract}), not only the package \
         version; the two answer different questions and can move independently:\n{all}"
    );
}
