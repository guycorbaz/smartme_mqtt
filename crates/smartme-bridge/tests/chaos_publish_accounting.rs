//! [#85] — **does the drop count balance across an ungraceful disconnect?**
//!
//! # The claim under measurement
//!
//! `AsyncClient::try_publish` answers `Ok` when a message enters `rumqttc`'s
//! request channel, **not** when it leaves the socket. Everything still sitting
//! in that channel is discarded when the `EventLoop` is dropped on reconnect, and
//! story 4.11 counts those readings as published. The manual states the
//! consequence rather than hiding it — *the counts are a floor on what was lost,
//! never a ceiling* — and [#85] asks for the size of the gap.
//!
//! # Why this file exists and `chaos_broker_recovery` does not answer it
//!
//! [#85] names that test as the place the figure becomes observable. **It is
//! not**, measured on 2026-08-30: its readings are handed over once
//! `stop_with_timeout` has returned, so the socket is already dead and every run
//! measures `("garage", "before-birth", 3)` — counted, not silently lost. The
//! window needs a connection that is **alive but dying**, which is a different
//! thing from a broker already down.
//!
//! So the disconnect here is forced the way `chaos_bdseq_per_connect` forces
//! one: an oversized frame on the node's NCMD topic, which `rumqttc` rejects
//! inside `poll()` — the socket drops from the inside while the session is
//! otherwise healthy, which is the only shape that can leave messages in the
//! request channel.
//!
//! # It measures an ACCOUNTING IDENTITY, not the window
//!
//! Catching the window deterministically would mean out-running a loopback
//! socket, which is a race this test does not control and should not pretend to.
//! What it can check is whether the books balance:
//!
//! ```text
//!     handed over  ==  received by an independent subscriber  +  counted as dropped
//! ```
//!
//! A left-hand side larger than the right is [#85] itself, in readings. The
//! figures are **printed on every run** and the gap is **not asserted to be
//! zero**: a test that pinned the number would pin this machine's timing, and one
//! that swept an empty set and called it coverage is the defect story 4.12's
//! first draft shipped.
//!
//! # The trap this file must not fall into, and it cost a year once
//!
//! **The saboteur's oversized frame reaches every subscriber on `spBv1.0/#`,
//! including the observer.** An observer whose incoming limit is below that frame
//! drops its own socket and misses everything in the window — which is how [#43]
//! stood from 2026-08-01 to 2026-08-29 on a measurement that was the instrument's
//! and not the bridge's. `common::named_subscriber_on` raises the limit for
//! exactly this reason; nothing here may build a private subscriber that does not.
//!
//! [#43]: https://github.com/guycorbaz/smartme_mqtt/issues/43
//! [#85]: https://github.com/guycorbaz/smartme_mqtt/issues/85

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use rumqttc::{AsyncClient, MqttOptions, QoS};
use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::mqtt_driver::{self, MqttConfig};
use smartme_bridge::app::poll_publish::DropReason;
use smartme_bridge::core::channel::MeterUpdate;
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::core::oracle::Verdict;
use smartme_bridge::domain::{
    DeviceIdentity, Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis,
};

const SERIAL: &str = "30000003";
const NODE_ID: &str = "ChaosAccounting";
const GROUP: &str = "ChaosAccountingGroup";
const METER: &str = "garage";
const READING_AT: i64 = 1_786_968_000_000;

/// Comfortably above the driver's `MAX_INCOMING_PACKET` (10 KiB).
const OVERSIZED: usize = 32 * 1024;

/// How many readings are handed over. Enough that the accounting is a sum rather
/// than a coin toss, and few enough that the run stays short.
const HANDED: i64 = 40;

struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn a_reading_at(value_date: i64) -> MeterUpdate {
    MeterUpdate::uniform(
        MeterId::new(METER),
        Measurement {
            meter: MeterId::new(METER),
            serial: Serial::new(SERIAL),
            power: Some(Kw(0.018)),
            energy: Some(Kwh(4_843.822 + value_date as f64 / 1_000_000.0)),
            value_date: UtcMillis(value_date),
            quality: Quality::Good,
        },
        Verdict::good(),
    )
}

async fn saboteur(port: u16) -> AsyncClient {
    let mut options = MqttOptions::new("saboteur-accounting", "127.0.0.1", port);
    options.set_keep_alive(Duration::from_secs(5));
    options.set_max_packet_size(OVERSIZED * 2, OVERSIZED * 2);
    let (client, mut eventloop) = AsyncClient::new(options, 16);
    tokio::spawn(async move { while eventloop.poll().await.is_ok() {} });
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
}

/// One run. Returns `(distinct DDATA an independent subscriber received, readings
/// the bridge counted as dropped)`.
async fn run_accounting(sabotage: bool) -> (usize, u64) {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::named_subscriber(port, "accounting-observer").await;

    let state_dir = std::env::temp_dir().join(format!("chaos_accounting_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(GROUP, NODE_ID).expect("valid identifiers");
    let (tx, rx) = mpsc::channel(64);
    let (_death_tx, death_rx) = oneshot::channel();
    let (_device_tx, device_rx) = tokio::sync::mpsc::channel(4);
    let beats = smartme_bridge::app::poll_publish::Heartbeats::for_meters([MeterId::new(METER)]);

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
        vec![DeviceIdentity::new(
            MeterId::new(METER),
            Serial::new(SERIAL),
        )],
        Arc::clone(&clock),
        smartme_bridge::app::mqtt_driver::Health {
            meters: beats.clone(),
            sink: smartme_bridge::app::mqtt_driver::SinkHealth::new(),
        },
        rx,
        device_rx,
        death_rx,
    ));

    common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/DBIRTH/")
    })
    .await
    .expect("the device must be born before any of this counts");

    // ---- hand readings over, and break the socket in the middle -------------
    //
    // The break lands mid-stream ON PURPOSE: the messages already accepted into
    // `rumqttc`'s request channel at that instant are the ones [#85] is about,
    // and they only exist while the session is healthy.
    let breaker = saboteur(port).await;
    for n in 0..HANDED {
        tx.send(a_reading_at(READING_AT + n * 1_000))
            .await
            .expect("the driver is listening");
        if n == HANDED / 2 && sabotage {
            breaker
                .publish(
                    format!("spBv1.0/{GROUP}/NCMD/{NODE_ID}"),
                    QoS::AtLeastOnce,
                    false,
                    vec![0u8; OVERSIZED],
                )
                .await
                .expect("the saboteur can publish");
        }
    }

    // Long enough for the reconnect (a 1 s floor plus jitter) and the re-birth.
    tokio::time::sleep(Duration::from_secs(12)).await;

    // ---- the books ---------------------------------------------------------
    //
    // **THE DRAIN'S BOUNDARY IS A MEASURED SILENCE, not a momentary gap**, and the
    // first version of this file got that wrong. `while let Ok(_) = try_recv()`
    // stops at the first instant the observer task has not yet forwarded — which
    // on a burst of forty is almost immediately, and it reported 2 received out of
    // 40 handed over. That number was the drain's, not the bridge's.
    //
    // It is the same lesson `chaos_broker_recovery` records for one run in
    // twenty-three, and the third time in this session that an instrument was the
    // culprit. Waiting for a full second of nothing is what makes the boundary
    // real.
    let mut received = HashSet::new();
    let mut every_topic: Vec<String> = Vec::new();
    while let Ok(Some(s)) = tokio::time::timeout(Duration::from_secs(1), seen.recv()).await {
        every_topic.push(s.topic.rsplit('/').nth(1).unwrap_or(&s.topic).to_string());
        if s.topic.contains("/DDATA/")
            && let Some(t) = s.payload.timestamp
        {
            // The payload timestamp IS the reading's `ValueDate` (ADR 0013), so it
            // identifies which reading arrived — the one place that deviation
            // makes a measurement easier rather than harder.
            received.insert(t as i64);
        }
    }
    let fleet = beats.snapshot();
    let dropped: u64 = fleet
        .dropped()
        .into_iter()
        .filter(|lost| lost.reason != DropReason::UndrainedAtShutdown)
        .map(|lost| lost.count)
        .sum();

    let accounted = received.len() as u64 + dropped;
    println!(
        "\n=== [#85] accounting — {} ===",
        if sabotage {
            "socket dropped mid-stream"
        } else {
            "CONTROL, a healthy session"
        }
    );
    {
        let mut by_kind = std::collections::BTreeMap::new();
        for k in &every_topic {
            *by_kind.entry(k.clone()).or_insert(0_usize) += 1;
        }
        println!("  everything the observer saw: {by_kind:?}");
    }
    println!("  handed to the driver          : {HANDED}");
    println!("  distinct DDATA an observer got: {}", received.len());
    println!("  counted as dropped            : {dropped}");
    println!("  accounted for                 : {accounted}");
    println!(
        "  UNACCOUNTED (this is [#85])   : {}",
        HANDED as u64 - accounted.min(HANDED as u64)
    );
    println!(
        "  by reason: {:?}",
        fleet
            .dropped()
            .into_iter()
            .map(|l| (l.reason.as_str(), l.count))
            .collect::<Vec<_>>()
    );

    driver.abort();
    (received.len(), dropped)
}

/// **The control, and it is what makes the measurement below mean anything.**
///
/// A healthy session must account for every reading exactly: what the bridge was
/// handed is what a third party received. This is an invariant and is asserted as
/// one — if it ever fails, the harness is broken and nothing measured beside it
/// can be believed.
///
/// It exists because the first version of this file had no control and reported
/// `2 received out of 40` for a run whose drain was the culprit. A measurement
/// without a control measures the instrument.
#[tokio::test(flavor = "multi_thread")]
async fn chaos_a_healthy_session_accounts_for_every_reading() {
    let (received, dropped) = run_accounting(false).await;
    assert_eq!(
        received as i64, HANDED,
        "a session nobody disturbed must deliver every reading it was handed; \
         anything else is the harness and not the bridge"
    );
    assert_eq!(dropped, 0, "and it must count no loss");
}

/// **[#85], measured.** The same run, with the socket dropped from inside
/// `poll()` mid-stream.
///
/// # What the measurement says, 2026-08-30
///
/// Against the control above — 40 handed, 40 received, 0 dropped — the sabotaged
/// run measured **40 handed, 1 received, 0 counted as dropped**. Thirty-nine
/// readings were accepted by `try_publish`, reported as handed over, and reached
/// nobody, while `dropped_readings` said zero.
///
/// **That is larger than [#85] supposed.** The issue reasons about *"everything
/// still sitting in that channel"* as a bounded remainder; the measurement shows
/// the bound is the channel itself, because a burst is accepted whole while the
/// socket is dying. The manual's *"a floor on what was lost, never a ceiling"* is
/// exact, and the ceiling is every reading in flight.
///
/// **What it does NOT say.** Forty readings in one microsecond is not production:
/// one meter at a 30-second period has one reading in flight, and a fleet has as
/// many as it has meters. The measurement establishes the MECHANISM and its
/// ceiling, not an expected loss.
///
/// # Why the gap is not asserted
///
/// It is a measurement of this machine's timing as much as of the bridge, and
/// pinning it would pin the harness. What is asserted is that the books never
/// claim MORE than was handed over, and that the run exercised a disconnect at
/// all — a healthy run would balance perfectly and prove nothing.
#[tokio::test(flavor = "multi_thread")]
async fn chaos_the_books_do_not_balance_across_an_ungraceful_disconnect() {
    let (received, dropped) = run_accounting(true).await;
    let accounted = received as u64 + dropped;

    assert!(
        accounted <= HANDED as u64,
        "the books account for more readings than were handed over ({accounted} > {HANDED}): \
         either a reading is counted twice or the wire carried one nobody sent"
    );
    assert!(
        received < HANDED as usize || dropped > 0,
        "every reading arrived and none was dropped, so no disconnect was exercised — this \
         measurement is of a healthy session and says nothing about [#85]"
    );
}
