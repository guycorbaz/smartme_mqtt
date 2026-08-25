//! Story 1.14 — the cloud goes silent while the node stays alive.
//!
//! This is the failure mode that motivates the whole project: the bridge is
//! connected and healthy, the broker is up, the SCADA host sees a live node —
//! and the data behind it stopped arriving. A bridge that keeps showing the
//! last value as current here is lying, and nothing downstream can tell.
//!
//! The assertion is made from an INDEPENDENT subscriber: what a third party
//! actually receives, not what the bridge believes it published. Its
//! deterministic twin is `poll_publish::tests::a_silent_cloud_times_out_into_
//! stale_instead_of_wedging`, which exercises the same timeout path with no
//! network at all.

mod common;

use std::time::Duration;

use smart_me_client::Credentials;
use smartme_bridge::adapters::sparkplug_publisher::{
    METRIC_ENERGY, METRIC_POWER, ignition_quality_code,
};
use smartme_bridge::app::{BridgeConfig, PollConfig};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{MeterId, Serial};

const SERIAL: &str = "30000001";

#[tokio::test(flavor = "multi_thread")]
async fn chaos_stale_on_cloud_timeout() {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::independent_subscriber(port).await;

    let state_dir = std::env::temp_dir().join(format!("chaos_cloud_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");

    let config = BridgeConfig {
        // TEST-NET-1 (RFC 5737): guaranteed unroutable, so the fetch times out
        // the way a silent cloud does rather than failing fast.
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
        node_id: "ChaosCloud".to_string(),
        broker_host: "127.0.0.1".to_string(),
        broker_port: port,
        bd_seq_path: state_dir.join("bdseq.toml"),
        poll: PollConfig {
            interval: Duration::from_millis(300),
            fetch_timeout: Duration::from_millis(500),
        },
        policy: Policy::DEFAULT,
        log_dir: None,
        log_keep: None,
        ui_port: None,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let bridge = tokio::spawn(async move {
        let _ = smartme_bridge::app::run(config, async {
            let _ = stop_rx.await;
        })
        .await;
    });

    // The device BIRTH is the bridge's first statement about this meter, and it
    // is made before any reading exists.
    let birth = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/DBIRTH/") && s.topic.ends_with(common::CONFIG_METER_ID)
    })
    .await
    .expect("the device birth reaches an independent subscriber");

    for metric in [METRIC_POWER, METRIC_ENERGY] {
        assert_eq!(
            birth.quality_of(metric),
            Some(ignition_quality_code(sparkplug_b::Quality::Stale)),
            "{metric} must be STALE before the first successful fetch"
        );
        assert!(
            !birth.has_value(metric),
            "{metric} must carry NO value: there is nothing to report yet"
        );
    }

    // Now let several poll intervals elapse with the cloud unreachable. The node
    // stays alive on the broker the whole time — and must never claim a good
    // reading it does not have.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let mut lies = Vec::new();
    while let Ok(s) = seen.try_recv() {
        for metric in [METRIC_POWER, METRIC_ENERGY] {
            let claims_good =
                s.quality_of(metric) == Some(ignition_quality_code(sparkplug_b::Quality::Good));
            if claims_good || (s.topic.contains("/DDATA/") && s.has_value(metric)) {
                lies.push(format!("{} claimed {metric} was usable", s.topic));
            }
        }
    }
    assert!(
        lies.is_empty(),
        "the cloud never answered, so nothing may be published as good:\n{}",
        lies.join("\n")
    );

    let _ = stop_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(10), bridge).await;
    let _ = std::fs::remove_dir_all(&state_dir);
}
