//! The startup banner must survive the failure it exists to explain.
//!
//! # Why this is a test and not a glance at the code
//!
//! A version line is trivially easy to write and trivially easy to place where
//! it does no good. The case an operator needs it in is the **crash-looping
//! container**: one whose configuration is wrong, which therefore exits before
//! doing anything, and whose entire output today is
//!
//! ```text
//! Error: "missing environment variable SMARTME_GROUP_ID"
//! ```
//!
//! A banner emitted after the configuration is read would be absent from exactly
//! that log. So the property under test is not *"the version is logged"* — it is
//! **"the version is logged even when the process refuses to start"**, and only
//! running the real binary can show that.
//!
//! Requested by Guy on 2026-08-01 after `v0.3.0` went into service, when the
//! recurring question during that day's work had been *"is this the published
//! image or the working tree?"* — a question the image tag cannot answer, since
//! the tag and the compiled version can drift. `CARGO_PKG_VERSION` is resolved at
//! compile time and cannot.

use std::process::Command;

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

/// Runs the bridge with a DELIBERATELY incomplete configuration and returns
/// everything it said.
///
/// The identity variables are the ones withheld, because they are the guard that
/// actually fires in a real misconfigured deployment — it fired on panoramix on
/// the day this test was written.
fn run_with_missing_identity() -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        // Everything EXCEPT the Sparkplug identity.
        .env("SMARTME_CLIENT_ID", "x")
        .env("SMARTME_CLIENT_SECRET", "x")
        .env("SMARTME_METER_ID", "m")
        .env("SMARTME_DEVICE_ID", "d")
        .env("SMARTME_SERIAL", "9")
        .env("SMARTME_BROKER_HOST", "192.0.2.1")
        .output()
        .expect("the bridge binary runs");
    (
        strip_ansi(&String::from_utf8_lossy(&out.stdout)),
        strip_ansi(&String::from_utf8_lossy(&out.stderr)),
        out.status.success(),
    )
}

#[test]
fn the_version_is_logged_even_when_the_bridge_refuses_to_start() {
    let (stdout, stderr, success) = run_with_missing_identity();
    let all = format!("{stdout}{stderr}");

    // The premise. If this ever stops holding, the test below would be asserting
    // the banner on a HEALTHY start, which is the easy case and not the one that
    // matters — so the premise is checked rather than assumed.
    assert!(
        !success,
        "the bridge started despite a missing Sparkplug identity; this test's whole \
         subject is what gets logged when it REFUSES to start:\n{all}"
    );
    assert!(
        all.contains("SMARTME_GROUP_ID"),
        "expected the identity guard to name the missing variable:\n{all}"
    );

    assert!(
        all.contains(env!("CARGO_PKG_VERSION")),
        "the package version {} does not appear anywhere in the output of a bridge that \
         refused to start. That is the log an operator reads when a container is \
         crash-looping, and it is the one case where knowing the build matters most:\n{all}",
        env!("CARGO_PKG_VERSION")
    );
    assert!(
        all.contains("smartme_mqtt starting"),
        "the startup banner is missing:\n{all}"
    );
}

#[test]
fn the_banner_precedes_the_failure() {
    let (stdout, stderr, _) = run_with_missing_identity();

    // ORDER, not mere presence. A banner that appears after the error would still
    // satisfy the test above if both landed in the same capture, and would still
    // be useless in a log read top-down during an incident.
    //
    // The two go to different streams — the banner is traced to stdout, the error
    // is returned from `main` and printed by the runtime to stderr — so their
    // relative order cannot be read from a single concatenation. What IS provable,
    // and what actually matters, is that the banner exists on stdout while the
    // process was still running, i.e. before the exit that produced the error.
    assert!(
        stdout.contains("smartme_mqtt starting"),
        "the banner must be on stdout, emitted while the process was still alive; \
         stdout was:\n{stdout}"
    );
    assert!(
        stderr.contains("SMARTME_GROUP_ID"),
        "the failure must be the one under test; stderr was:\n{stderr}"
    );
}

#[test]
fn the_banner_carries_the_contract_version() {
    let (stdout, stderr, _) = run_with_missing_identity();
    let all = format!("{stdout}{stderr}");

    // The contract version is what a CONSUMER sees. It answers a different
    // question from the package version — "what will this put on the wire?" —
    // and it is the first thing worth knowing when a tag looks wrong in Ignition.
    let contract = smartme_bridge::adapters::sparkplug_publisher::CONTRACT_VERSION;
    assert!(
        all.contains(&format!("contract={contract}")),
        "the banner must state the contract version ({contract}), not only the package \
         version; the two answer different questions and can move independently:\n{all}"
    );
}
