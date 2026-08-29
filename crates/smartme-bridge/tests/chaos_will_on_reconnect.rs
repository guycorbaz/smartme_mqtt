//! [#43] — the will DOES fire when the bridge reconnects, and the missing one
//! was the observer's own socket.
//!
//! # What #43 measured, and what it actually measured
//!
//! `chaos_bdseq_per_connect` recorded that across a forced disconnect an
//! independent subscriber sees `DBIRTH(session 1)`, `NBIRTH(session 2)` and **no
//! NDEATH at all**. Its stated hypothesis was session takeover: the bridge
//! reconnects under the same `client_id` inside the one-second floor and the new
//! CONNECT supersedes the half-open session before the broker publishes its will.
//!
//! **That hypothesis is refuted, and so is the observation.** Three explanations
//! were taken in turn on 2026-08-29 and only the last survived:
//!
//! - **the observer's QoS** — refuted by reading: `chaos_sigterm_no_lie` uses the
//!   SAME `common` subscriber at the same QoS and sees TWO certificates on every
//!   gate pass, the explicit death and the will;
//! - **a graceful DISCONNECT** — refuted by reading: the driver never sends one on
//!   either path, and says why (it instructs the broker to DISCARD the will);
//! - **session takeover** — refuted by MEASUREMENT, by this file: raising the
//!   reconnect floor to eight seconds changed nothing, and no will arrived.
//!
//! **The cause is the saboteur's own frame.** The disconnect is forced by
//! publishing 32 KiB to the node's NCMD topic; the observer subscribes to
//! `spBv1.0/#`, so that frame reaches the OBSERVER too. Until 2026-08-29
//! `common::named_subscriber_on` never raised `max_incoming_packet_size` from
//! `rumqttc`'s 10 KiB default, so the observer's own `poll()` rejected it, its
//! socket dropped, and — clean session, QoS 0, nothing queued — it was away at
//! the exact instant the will was published. It reconnected, re-subscribed, and
//! saw the next NBIRTH. Hence a `DBIRTH → NBIRTH` pair with nothing between them.
//!
//! **An instrument that is knocked out by the event it is measuring reports the
//! event as absent.** That is the general lesson, and it is the third time this
//! repository has met it: a `tracing` capture that stayed empty because no
//! subscriber existed, an observer window that saw no deaths Ignition counted,
//! and now this.
//!
//! # What this file guards
//!
//! That the will reaches a subscriber across a reconnect **at either floor** —
//! the production one and a raised one. The two runs together say the wait is
//! irrelevant, which is what closes the takeover hypothesis rather than merely
//! setting it aside.
//!
//! # FALSIFICATION
//!
//! **2026-08-29, run and observed:** restoring the observer's inherited limit —
//! deleting the `set_max_packet_size` call in `common::named_subscriber_on`,
//! which is exactly the state this file was written from — turns the run RED.
//! That is the ordinary shape of the fault: not a wrong constant, but a default
//! nobody overrode.
//!
//! **It goes red at the SECOND BIRTH, not at the will, and the difference is
//! worth the sentence.** A knocked-out observer misses everything in its window,
//! not only the message under test, so the first assertion it reaches is the one
//! about the reconnect — 61 s of timeouts rather than the clean *"no NDEATH"* I
//! had predicted. The prediction was wrong about WHERE it would fail, so both
//! failure messages now name the observer: whichever one a reader meets points
//! at the instrument rather than at the bridge.
//!
//! [#43]: https://github.com/guycorbaz/smartme_mqtt/issues/43

mod common;

use std::sync::Arc;
use std::time::Duration;

use rumqttc::{AsyncClient, MqttOptions, QoS};
use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::mqtt_driver::{self, MqttConfig};
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::domain::{DeviceIdentity, Serial};

const SERIAL: &str = "30000001";
const NODE_ID: &str = "ChaosWillReconnect";
const GROUP: &str = "ChaosWillGroup";

/// Comfortably above the driver's `MAX_INCOMING_PACKET` (10 KiB).
const OVERSIZED: usize = 32 * 1024;

/// What production uses.
const PRODUCTION_FLOOR: Duration = Duration::from_secs(1);

/// The raised floor — long enough that a broker holding the ended session for any
/// ordinary reason has finished with it, and short enough not to dominate the gate.
const LONG_FLOOR: Duration = Duration::from_secs(8);

struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn saboteur(port: u16) -> AsyncClient {
    let mut options = MqttOptions::new("saboteur-will", "127.0.0.1", port);
    options.set_keep_alive(Duration::from_secs(5));
    options.set_max_packet_size(OVERSIZED * 2, OVERSIZED * 2);
    let (client, mut eventloop) = AsyncClient::new(options, 16);
    tokio::spawn(async move { while eventloop.poll().await.is_ok() {} });
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
}

/// One forced disconnect at a given floor. Returns the `bdSeq` of the will the
/// observer saw before the reconnect, if any.
async fn will_seen_at(floor: Duration, tag: &str) -> (Option<i64>, i64, i64) {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::named_subscriber(port, &format!("will-observer-{tag}")).await;

    let state_dir = std::env::temp_dir().join(format!("chaos_will_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(GROUP, NODE_ID).expect("valid identifiers");
    let (_tx, rx) = mpsc::channel(64);
    let (_death_tx, death_rx) = oneshot::channel();
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
            // THE ONE VARIABLE between the two runs.
            reconnect_floor: floor,
        },
        node,
        vec![DeviceIdentity::new(
            smartme_bridge::domain::MeterId::new("garage"),
            Serial::new(SERIAL),
        )],
        Arc::clone(&clock),
        smartme_bridge::app::mqtt_driver::Health {
            meters: smartme_bridge::app::poll_publish::Heartbeats::default(),
            sink: smartme_bridge::app::mqtt_driver::SinkHealth::new(),
        },
        rx,
        device_rx,
        death_rx,
    ));

    let first = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await
    .expect("the bridge must birth on its first connection");
    let first_bd_seq = first.bd_seq().expect("a birth carries its session number");

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

    // The will first: at the raised floor it can only be the ended session's,
    // because the next CONNECT has not happened yet.
    let will = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/NDEATH/")
    })
    .await;
    let will_bd_seq = will.as_ref().and_then(|s| s.bd_seq());

    let second = common::wait_for(&mut seen, Duration::from_secs(40), |s| {
        s.topic.contains("/NBIRTH/") && s.bd_seq() != Some(first_bd_seq)
    })
    .await
    .expect(
        "no second birth under a NEW session number arrived. A raised floor lengthens the wait; \
         it must not prevent the reconnect. CHECK THE OBSERVER FIRST: the saboteur's oversized \
         frame reaches it too, and an observer whose incoming limit is below that frame drops \
         its own socket and misses EVERYTHING in the window — not only the will. That is what \
         #43 recorded for a year. See `common::named_subscriber_on`",
    );
    let second_bd_seq = second.bd_seq().expect("a birth carries its session number");

    driver.abort();
    (will_bd_seq, first_bd_seq, second_bd_seq)
}

#[tokio::test(flavor = "multi_thread")]
async fn chaos_the_will_reaches_a_subscriber_across_a_reconnect_at_either_floor() {
    for (floor, tag) in [(PRODUCTION_FLOOR, "floor"), (LONG_FLOOR, "raised")] {
        let (will, first, second) = will_seen_at(floor, tag).await;

        assert_eq!(
            second,
            first + 1,
            "{tag}: the session number must advance by exactly one per CONNECT"
        );
        let will = will.unwrap_or_else(|| {
            panic!(
                "{tag} ({floor:?}): no NDEATH reached the observer between the drop and the \
                 reconnect. Before checking the bridge, check the OBSERVER: this is exactly \
                 what #43 recorded for a year, and the cause was that the saboteur's 32 KiB \
                 frame also reached the observer, whose inherited 10 KiB limit dropped its \
                 own socket. See `common::named_subscriber_on`"
            )
        });
        assert_eq!(
            will, first,
            "{tag}: the will must carry the ENDED session's number, not the new one. A \
             certificate for a session that had not begun is a worse fault than a missing one"
        );
    }
}
