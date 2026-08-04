//! Story 5.3 AC1 — an unconfirmed mapping opens no MQTT session at all.
//!
//! # Why a broker, and not a unit test
//!
//! The claim is about what a third party sees, and the failure this guards
//! against is the *partial* one: withholding the DDATA while still connecting
//! and birthing. That would look correct from inside the process — nothing
//! published, no values on the wire — and would already have done the
//! irreversible half, because an NBIRTH alone creates the node's folder in a
//! Sparkplug host's tag tree. Only a subscriber can tell the two apart.
//!
//! # The guard, and it is the reason this file has two tests
//!
//! *"No NBIRTH appeared"* holds trivially over a broker nothing ever connected
//! to — the shape that has already produced a false pass here. So the same
//! harness, the same broker, the same configuration is first shown to produce a
//! birth when the mapping IS confirmed. Only then does its silence mean
//! anything.

mod common;

use std::time::Duration;

use smartme_bridge::app::store::{self, StoredConfig, StoredMeter};

const SERIAL: &str = "30000001";

fn state_dir(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("smartme_unconfirmed_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

fn stored(port: u16, group: &str) -> StoredConfig {
    StoredConfig {
        schema_version: store::SCHEMA_VERSION,
        group_id: group.to_string(),
        node_id: group.to_string(),
        broker_host: "127.0.0.1".to_string(),
        broker_port: port,
        publish_period_secs: 30,
        // TEST-NET-1 (RFC 5737): unroutable, so the cloud stays silent. The
        // Sparkplug session does not depend on having a reading.
        api_base: Some("https://192.0.2.1".to_string()),
        log_dir: None,
        log_keep: None,
        // `save` discards this and computes it — which is the rule under test in
        // `store`, and the reason each case below confirms explicitly.
        mapping_confirmed: false,
        meters: vec![StoredMeter {
            meter_id: "garage".to_string(),
            device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000001".to_string(),
            serial: SERIAL.to_string(),
            enabled: true,
        }],
    }
}

/// Spawn **the real binary** against `dir`, and leave it running.
///
/// An earlier draft of this file re-implemented `main.rs`'s decision here — it
/// read `mapping_confirmed` and branched itself — and a comment claimed that was
/// somehow testing the product. It was not: the branch under test WAS the branch
/// the test supplied, so the one line this story adds to `main.rs` could have
/// been deleted with both cases still green. Spawning the binary is the whole
/// point; there is no cheaper way to test a decision that lives in `main`.
fn spawn(dir: &std::path::Path) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", dir)
        .env("SMARTME_CLIENT_ID", "x")
        .env("SMARTME_CLIENT_SECRET", "x")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the bridge binary runs")
}

/// THE PREMISE. Without it, the silence asserted below would also be the silence
/// of a broker nothing ever reached.
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_mapping_does_birth_on_this_very_harness() {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::independent_subscriber(port).await;
    let dir = state_dir("confirmed");

    store::save(&dir, &stored(port, "ChaosConfirmed")).expect("save");
    store::confirm(&dir).expect("a human confirms the mapping");

    let mut bridge = spawn(&dir);

    let birth = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await;
    assert!(
        birth.is_some(),
        "a CONFIRMED mapping must reach the broker, or the absence asserted in \
         the other test proves only that this harness sees nothing"
    );

    let _ = bridge.kill();
    let _ = bridge.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

/// AC1 — and only now does the absence mean something.
///
/// FALSIFIED 2026-08-04 by disabling the guard in `main.rs`. The failure names
/// what reached the wire, which is the whole point of using a subscriber:
///
/// ```text
/// an unconfirmed mapping put Some("spBv1.0/ChaosUnconfirmed/NBIRTH/ChaosUnconfirmed")
/// on the wire. Nothing may be published before a human has said the mapping is
/// right — an NBIRTH alone creates a tag folder that outlives the process
/// ```
///
/// An NBIRTH, note — not a DDATA. A guard that only withheld the data would have
/// let exactly this through and looked correct from inside the process.
#[tokio::test(flavor = "multi_thread")]
async fn an_unconfirmed_mapping_never_reaches_the_broker() {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::independent_subscriber(port).await;
    let dir = state_dir("unconfirmed");

    // Saved and NOT confirmed. `save` computes the flag, so this is the state a
    // first save through the UI actually produces.
    store::save(&dir, &stored(port, "ChaosUnconfirmed")).expect("save");
    assert!(
        !store::read(&dir).expect("read").mapping_confirmed,
        "the premise of this test is that the mapping is unconfirmed"
    );

    let mut bridge = spawn(&dir);

    // NOTHING, not merely no DDATA. An NBIRTH alone creates the node folder a
    // host keeps, so waiting only for data would let the irreversible half pass.
    let anything = common::wait_for(&mut seen, Duration::from_secs(10), |s| {
        s.topic.contains("ChaosUnconfirmed")
    })
    .await;
    assert!(
        anything.is_none(),
        "an unconfirmed mapping put {:?} on the wire. Nothing may be published \
         before a human has said the mapping is right — an NBIRTH alone creates \
         a tag folder that outlives the process and is deleted by hand",
        anything.map(|s| s.topic)
    );

    let _ = bridge.kill();
    let _ = bridge.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
