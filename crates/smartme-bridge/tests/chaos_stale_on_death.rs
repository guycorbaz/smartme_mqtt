//! Story 1.14 — the bridge dies and the SCADA host is told.
//!
//! The transport-level half of the two-mechanism staleness guarantee: when the
//! bridge stops without warning, the broker publishes the death certificate it
//! was handed at connect time, and an independent subscriber sees the node go
//! down. Without this, a killed bridge leaves its last values on screen forever,
//! looking exactly like a healthy meter reading a steady load.
//!
//! The certificate must also be ATTRIBUTABLE: its `bdSeq` has to match the birth
//! it belongs to, or a consumer pairing death to birth will ignore it and keep
//! showing the frozen value as live.

mod common;

use std::time::Duration;

use smart_me_client::Credentials;
use smartme_bridge::app::{BridgeConfig, PollConfig};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{MeterId, Serial};

const SERIAL: &str = "30000001";

#[tokio::test(flavor = "multi_thread")]
async fn chaos_stale_on_death() {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::independent_subscriber(port).await;

    let state_dir = std::env::temp_dir().join(format!("chaos_death_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");

    let config = BridgeConfig {
        api_base: "https://192.0.2.1".to_string(),
        credentials: Credentials::Basic {
            user: "u".to_string(),
            password: "p".to_string(),
        },
        http_timeout: Duration::from_secs(30),
        meters: vec![smartme_bridge::app::config::MeterConfig {
            priority: false,
            meter: MeterId::new("garage"),
            device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000001".to_string(),
            serial: Serial::new(SERIAL),
            enabled: true,
        }],
        group_id: "Site".to_string(),
        node_id: "ChaosDeath".to_string(),
        broker_host: "127.0.0.1".to_string(),
        broker_port: port,
        bd_seq_path: state_dir.join("bdseq.toml"),
        poll: PollConfig {
            interval: Duration::from_millis(500),
            fetch_timeout: Duration::from_millis(500),
        },
        policy: Policy::DEFAULT,
        log_dir: None,
        log_keep: None,
        ui_port: None,
    };

    // The bridge runs with no shutdown signal: it will be killed, not asked.
    let bridge = tokio::spawn(async move {
        let _ = smartme_bridge::app::run(config, std::future::pending::<()>()).await;
    });

    let birth = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await
    .expect("the node birth reaches an independent subscriber");
    let birth_bd_seq = birth.bd_seq().expect("a birth carries its session number");

    // Kill it. No graceful stop, no chance to publish anything: exactly what a
    // power cut or an OOM kill looks like from the broker's side.
    bridge.abort();

    let death = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NDEATH/")
    })
    .await
    .expect("the broker publishes the will when the bridge dies without warning");

    assert_eq!(
        death.bd_seq(),
        Some(birth_bd_seq),
        "the death must be attributable to the session that was born, or a \
         consumer pairing them by bdSeq will ignore it and keep the frozen \
         value on screen"
    );
    assert_eq!(
        death.payload.seq, None,
        "a death carries no sequence number: the broker publishes it at a \
         moment the node cannot number"
    );

    let _ = std::fs::remove_dir_all(&state_dir);
}
