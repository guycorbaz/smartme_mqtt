//! Story 4.10 — a new session number on every CONNECT, seen from outside.
//!
//! # What this proves that no unit test can
//!
//! `bdSeq` is a promise made to a *consumer*: it is what lets a host pair a
//! death certificate to the birth it belongs to. A test that asks the publisher
//! what number it holds proves the bridge agrees with itself. This one asks an
//! independent subscriber what actually arrived on the wire, across a real
//! disconnect, against a real broker.
//!
//! # How the disconnect is forced, and why this mechanism
//!
//! By publishing an oversized frame to the node's NCMD topic. `rumqttc` rejects
//! any incoming packet above `max_incoming_packet_size` inside `poll()`, which
//! returns `Err` and drops the socket **ungracefully** — exactly the shape of a
//! real transport failure, and a path this driver already documents at length.
//! Restarting the broker container would work too, but it would also destroy the
//! observer's own subscription, and then the test would be measuring its own
//! plumbing.
//!
//! # Why the old behaviour would pass a weaker test
//!
//! Before Story 4.10 the bridge reconnected under the SAME `bdSeq`, because
//! `rumqttc` reconnects internally and rebuilds the CONNECT packet from the
//! `MqttOptions` captured at construction — so the registered will could never be
//! updated, and advancing the number would have left the broker holding a
//! certificate for a session that no longer existed. A test asserting only "a
//! second NBIRTH arrives" was therefore green before and after the change, and
//! would have proved nothing. The assertion is on the VALUE.

mod common;

use std::sync::Arc;
use std::time::Duration;

use rumqttc::{AsyncClient, MqttOptions, QoS};
use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::mqtt_driver::{self, MqttConfig};
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::domain::{DeviceIdentity, Serial};

const SERIAL: &str = "30000001";
const NODE_ID: &str = "ChaosBdSeq";
const GROUP: &str = "ChaosBdSeqGroup";

/// Comfortably above the driver's `MAX_INCOMING_PACKET` (10 KiB), so `poll()`
/// rejects the frame rather than delivering it.
const OVERSIZED: usize = 32 * 1024;

struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A client that exists only to break the bridge's session.
async fn saboteur(port: u16) -> AsyncClient {
    let mut options = MqttOptions::new("saboteur", "127.0.0.1", port);
    options.set_keep_alive(Duration::from_secs(5));
    // It must be allowed to SEND a frame far larger than the bridge will accept.
    options.set_max_packet_size(OVERSIZED * 2, OVERSIZED * 2);
    let (client, mut eventloop) = AsyncClient::new(options, 16);
    tokio::spawn(async move { while eventloop.poll().await.is_ok() {} });
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
}

#[tokio::test(flavor = "multi_thread")]
async fn chaos_bd_seq_advances_on_every_connect() {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::named_subscriber(port, "bdseq-observer").await;

    let state_dir = std::env::temp_dir().join(format!("chaos_bdseq_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(GROUP, NODE_ID).expect("valid identifiers");
    let (_tx, rx) = mpsc::channel(64);
    let (death_tx, death_rx) = oneshot::channel();

    let (_device_tx, device_rx) = tokio::sync::mpsc::channel(4);
    let driver = tokio::spawn(mqtt_driver::run(
        MqttConfig {
            client_id: NODE_ID.to_string(),
            host: "127.0.0.1".to_string(),
            port,
            keep_alive: Duration::from_secs(30),
            bd_seq_path: state_dir.0.join("bdseq.toml"),
            capacity: 64,
            death_flush: Duration::from_secs(2),
            reconnect_floor: Duration::from_secs(1),
        },
        node,
        // Both names since contract v13 (ADR 0049): the topic carries the
        // published one, the declaration is filed under the serial.
        vec![DeviceIdentity::new(
            smartme_bridge::domain::MeterId::new("garage"),
            Serial::new(SERIAL),
        )],
        Arc::clone(&clock),
        // Story 4.11's drop counters. EMPTY ON PURPOSE: this test asserts nothing
        // about lost readings, and `Heartbeats::dropped` skips a meter it does not
        // serve rather than panicking — so an empty fleet here counts nothing and
        // changes nothing. A test that wants the counts must build one for its own
        // meters.
        smartme_bridge::app::mqtt_driver::Health {
            meters: smartme_bridge::app::poll_publish::Heartbeats::default(),
            sink: smartme_bridge::app::mqtt_driver::SinkHealth::new(),
        },
        rx,
        // AC4's reconfiguration channel. These tests never send on it; the
        // sender is kept alive so the driver's branch stays armed rather than
        // disarming on a dropped end.
        device_rx,
        death_rx,
    ));

    // ---- the first session -------------------------------------------------
    let first = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await
    .expect("the bridge must birth on its first connection");
    let first_bd_seq = first.bd_seq().expect("a birth carries its session number");

    // ---- break the transport ----------------------------------------------
    let breaker = saboteur(port).await;
    breaker
        .publish(
            format!("spBv1.0/{GROUP}/NCMD/{NODE_ID}"),
            QoS::AtLeastOnce,
            false,
            vec![0u8; OVERSIZED],
        )
        .await
        .expect("the saboteur can publish");

    // ---- the session that follows -----------------------------------------
    let second = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NBIRTH/") && s.bd_seq() != Some(first_bd_seq)
    })
    .await
    .expect(
        "no second birth under a NEW session number arrived within 30 s. Either the bridge did \
         not reconnect at all, or it reconnected under the SAME bdSeq — which is the pre-4.10 \
         behaviour, and the thing this test exists to forbid",
    );
    let second_bd_seq = second.bd_seq().expect("a birth carries its session number");

    assert_eq!(
        second_bd_seq,
        first_bd_seq + 1,
        "the session number must advance by exactly one per CONNECT \
         (first {first_bd_seq}, second {second_bd_seq})"
    );

    // ---- what the WILL could not be made to prove HERE, and why -----------
    //
    // AC2 asks for "the NDEATH the broker holds", i.e. the WILL, observed from
    // outside. This test does not obtain it, and until 2026-08-29 it recorded
    // that as a MEASUREMENT: *"no NDEATH reaches a subscriber at all when the
    // bridge reconnects"*, twice on two separate drops, raised as [#43] with
    // session takeover as its hypothesis.
    //
    // **That measurement was an artefact of the instrument, and this file's own
    // saboteur is what caused it.** The 32 KiB frame published above to force the
    // disconnect reaches the OBSERVER too — it subscribes to `spBv1.0/#` — and
    // `common::named_subscriber_on` inherited `rumqttc`'s 10 KiB incoming
    // default, so the observer's own `poll()` rejected the frame, its socket
    // dropped, and it was away (clean session, QoS 0, nothing queued) at the
    // instant the will was published.
    //
    // The observer now takes a limit large enough for any frame a test can
    // produce, and `chaos_will_on_reconnect` shows the will arriving at BOTH the
    // production floor and a raised one — so the wait is irrelevant and takeover
    // is refuted. **An instrument knocked out by the event it measures reports
    // the event as absent**, which is the third time this repository has met that
    // shape.
    //
    // This test still asserts only the explicit death, because that is what its
    // own sequence produces; the will belongs to the file written for it.
    let _ = death_tx.send(());
    let death = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/NDEATH/")
    })
    .await
    .expect("a shutdown must produce a death certificate");

    assert_eq!(
        death.bd_seq(),
        Some(second_bd_seq),
        "the death certificate carries a different session than the one that was live \
         ({second_bd_seq}). A consumer pairing death to birth by bdSeq would discard it and keep \
         showing the last value as live",
    );

    let _ = tokio::time::timeout(Duration::from_secs(10), driver).await;
}
