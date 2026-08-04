//! Story 5.2 AC1 — the configuration survives a restart **and an image update**.
//!
//! # Two claims, and only one of them is cheap
//!
//! A restart re-reads a file the *same* binary wrote. An image update replaces
//! the binary that reads it, and that is what FR40 promises the operator: pull a
//! new tag, keep your settings. The two are not the same test, and the second is
//! the one a schema change breaks.
//!
//! # What stands in for "a newer binary"
//!
//! Not a second build — that would need two toolchain runs in CI to prove a
//! property that lives entirely in the file format. What actually distinguishes
//! an image update from a restart is that **the code reading the file may not be
//! the code that wrote it**, so what has to be tested is the file: a document
//! this build did not produce, read by this build.
//!
//! So the file is written as *text*, by hand, the way a different version would
//! have left it — never through `store::save`. A round-trip through the writer
//! would prove only that the writer and the reader agree with each other, which
//! is exactly the assurance an image update removes.
//!
//! Three shapes, and the third is the one that matters:
//!
//! 1. the same schema, written by hand: every setting still in force;
//! 2. the same schema with optional keys omitted: their documented defaults, not
//!    silence;
//! 3. a **different** schema version: refused, loudly, rather than read by
//!    guesswork.

use smartme_bridge::app::store::{self, Credential, SCHEMA_VERSION};

fn state_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("smartme_update_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

fn credential() -> Credential {
    Credential {
        client_id: Some("id".into()),
        client_secret: Some("secret".into()),
    }
}

/// A file this build did not write, spelled out the way another version would
/// have left it on the volume.
fn hand_written(version: u32, optional: &str) -> String {
    format!(
        "# written by a different build of smartme_mqtt\n\
         schema_version = {version}\n\
         group_id = \"Plant\"\n\
         node_id = \"Bridge01\"\n\
         broker_host = \"mqtt.example.lan\"\n\
         broker_port = 8883\n\
         publish_period_secs = 45\n\
         {optional}\n\
         [[meters]]\n\
         meter_id = \"garage\"\n\
         device_id = \"a1a1a1a1-b2b2-c3c3-d4d4-000000000001\"\n\
         serial = \"9202685\"\n\
         enabled = true\n"
    )
}

/// AC1 — the settings a previous version persisted are still in force under this
/// one, down to the values.
///
/// FALSIFIED 2026-08-04 by dropping one stored value on the way through
/// (`publish_period_secs: None` in `into_raw`) — the shape a renamed key
/// produces, and the exact failure FR40 exists to prevent:
///
/// ```text
/// assertion `left == right` failed
///   left: 30
///  right: 45
/// ```
///
/// 30 is the default. The bridge would have run, published, and looked healthy,
/// on a period the operator never chose.
#[test]
fn every_setting_written_by_another_build_is_still_in_force() {
    let dir = state_dir("carried");
    std::fs::write(
        store::config_path(&dir),
        hand_written(SCHEMA_VERSION, "log_dir = \"/data/logs\"\nlog_keep = 30\n"),
    )
    .expect("write");

    let raw = store::load(&dir, credential()).expect("a file from another build must load");
    let config =
        smartme_bridge::app::config::validate(raw).expect("and must pass this build's validation");

    // Every value, not a spot check: "it loaded" would also be true of a reader
    // that silently substituted its own defaults for everything.
    assert_eq!(config.group_id, "Plant");
    assert_eq!(config.node_id, "Bridge01");
    assert_eq!(config.broker_host, "mqtt.example.lan");
    assert_eq!(config.broker_port, 8883);
    assert_eq!(config.poll.interval.as_secs(), 45);
    assert_eq!(config.log_dir.as_deref(), Some("/data/logs"));
    assert_eq!(config.log_keep, Some(30));
    assert_eq!(config.meters.len(), 1);
    assert_eq!(config.meters[0].serial.as_str(), "9202685");
    assert!(config.meters[0].enabled);

    let _ = std::fs::remove_dir_all(&dir);
}

/// An older file may simply not have the optional keys. They must take their
/// DOCUMENTED defaults — which is a different claim from "they are absent".
#[test]
fn optional_keys_a_previous_build_never_wrote_take_their_documented_defaults() {
    let dir = state_dir("defaults");
    std::fs::write(store::config_path(&dir), hand_written(SCHEMA_VERSION, "")).expect("write");

    let raw = store::load(&dir, credential()).expect("load");
    let config = smartme_bridge::app::config::validate(raw).expect("validate");

    assert_eq!(
        config.api_base, "https://api.smart-me.com",
        "an absent api_base must take the documented default, not an empty string"
    );
    assert_eq!(config.log_dir, None, "absent log_dir means console only");
    assert_eq!(
        config.log_keep, None,
        "and the retention default applies later"
    );
    // The required values are still carried, so the defaults above are not
    // standing in for a file that failed to parse.
    assert_eq!(config.poll.interval.as_secs(), 45);

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The claim AC1 actually rests on.** A newer binary meeting an older layout
/// must refuse, not guess.
///
/// This is the case a restart can never produce and only an image update can:
/// the writer and the reader are different code. Serde's default behaviour is
/// the trap — an unknown key ignored, a missing one defaulted — so a renamed
/// setting would read as absent, take its default, and the bridge would run
/// happily on a configuration nobody wrote.
///
/// FALSIFIED 2026-08-04 by disabling the version comparison in `store::read`:
///
/// ```text
/// test a_file_from_a_different_schema_is_refused_and_says_why ... FAILED
/// a file from another schema must be refused, not read by guesswork
/// ```
///
/// Note which tests stayed GREEN under that mutation: the three others, because
/// a file from another schema still parses perfectly well. That is the whole
/// danger, and why this case has a test of its own.
#[test]
fn a_file_from_a_different_schema_is_refused_and_says_why() {
    let dir = state_dir("older");
    std::fs::write(
        store::config_path(&dir),
        hand_written(SCHEMA_VERSION - 1, ""),
    )
    .expect("write");

    let errors = store::load(&dir, credential())
        .expect_err("a file from another schema must be refused, not read by guesswork");
    let rendered = format!("{errors}");

    assert!(
        rendered.contains(&format!("schema version {}", SCHEMA_VERSION - 1)),
        "the refusal must name the version it FOUND, or an operator cannot tell \
         which image wrote the file: {rendered}"
    );
    assert!(
        rendered.contains("no migration"),
        "and it must say why it is not simply read anyway: {rendered}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The restart half, kept honest by being stated separately.
///
/// It is the cheap one and it is not what FR40 promises — but leaving it out
/// would make the file above look like the whole of AC1 when it is the harder
/// half only.
#[test]
fn a_configuration_this_build_wrote_survives_being_read_again() {
    let dir = state_dir("restart");
    std::fs::write(store::config_path(&dir), hand_written(SCHEMA_VERSION, "")).expect("write");

    let first = store::read(&dir).expect("read");
    store::save(&dir, &first).expect("rewrite it as this build would");
    let second = store::read(&dir).expect("read back");

    assert_eq!(
        first, second,
        "a configuration must not drift by being written and read by the same build"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
