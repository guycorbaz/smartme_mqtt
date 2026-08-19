//! Story 4.12 AC1 — nothing is re-timestamped when the link comes back.
//!
//! # What this proves that a unit test cannot
//!
//! `an_hour_of_outage_does_not_move_the_re_declared_reading_forward` drives the
//! publisher directly and asserts the same property. This one asserts it **from
//! outside the process**, on the bytes an independent subscriber actually
//! received, across a real transport break and a real reconnection — so it
//! covers the encode, the session lifecycle and the driver's re-birth path
//! together, none of which the unit test touches.
//!
//! # The break is a session takeover, and story 4.13 owns the other one
//!
//! A saboteur publishes a frame larger than the driver will accept, so `poll()`
//! rejects it and the transport drops. That is a reconnect **without stopping
//! the broker**, which keeps this test fast and hermetic.
//!
//! **Story 4.13 (`chaos_broker_recovery`) owns the proof with a broker container
//! stopped and restarted.** That is a different code path to the same
//! reconnection — the broker is gone rather than the socket — and neither test
//! may be cited as the other's evidence. Written here so the boundary is not
//! blurred later by someone counting green tests.
//!
//! # The trap this test could fall into, and how it does not
//!
//! Asserting only "a DBIRTH arrived after the reconnect" would pass against a
//! bridge that stamped the reconnection instant on it — which is the whole defect
//! FR22's anti-replay clause exists to forbid. **The assertion is on the VALUE**,
//! and the reading's `ValueDate` is set far in the past so that a bridge
//! stamping `now` cannot coincidentally agree with it.
//!
//! # Falsification — 2026-08-18, two mutations RUN, output copied
//!
//! Moving `Emission::DeviceBirthRedeclaring` to `PublicationInstant` goes red on
//! the re-declaration with `THE RE-DECLARED READING FOLLOWED THE CLOCK … left:
//! 1787078552364, right: 1786968000000` — the observed stamp being the real
//! reconnection instant, an hour and more past the reading.
//!
//! Making the DDATA stamp a fixed instant goes red on the reading published
//! AFTER the reconnect: `a reading acquired AFTER the reconnection carries its
//! own acquisition time too … left: Some(1786968000000), right:
//! Some(1786968060000)`.
//!
//! **The NBIRTH's publication instant — added by the 2026-08-19 review, mutation
//! RUN.** Replacing `clock.wall()` with `UtcMillis(42)` at both `announce` call
//! sites (`mqtt_driver.rs:1223` and `:1292`) — [#30]'s prescription word for word —
//! goes red with `THE NODE BIRTH DOES NOT SPEAK THE PUBLICATION INSTANT. It was
//! published somewhere in [1787132581436, 1787132581477] and carries 42`. **Before
//! that assertion the same mutation left the WHOLE suite green**: 258 unit tests,
//! and every chaos test that observes an NBIRTH, this one included.
//!
//! **The second reading exists because of the first draft's own failure.** This
//! test ended with a sweep over "whatever else arrived", and it swept ZERO
//! messages — an assertion over an empty set, scored as coverage. The count was
//! printed, which is how it was caught. Sending a second reading makes the
//! post-reconnect path something the test exercises rather than hopes to observe.

mod common;

use std::sync::Arc;
use std::time::Duration;

use rumqttc::{AsyncClient, MqttOptions, QoS};
use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::mqtt_driver::{self, MqttConfig};
use smartme_bridge::app::poll_publish::Heartbeats;
use smartme_bridge::core::channel::MeterUpdate;
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::core::oracle::Verdict;
use smartme_bridge::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};

const SERIAL: &str = "30000001";
const NODE_ID: &str = "ChaosNoReplay";
const GROUP: &str = "ChaosNoReplayGroup";

/// Comfortably above the driver's `MAX_INCOMING_PACKET` (10 KiB), so `poll()`
/// rejects the frame rather than delivering it.
const OVERSIZED: usize = 32 * 1024;

/// **2026-08-18T12:00:00Z, and it is deliberately nowhere near `now`.**
///
/// A bridge stamping the publication instant would emit a number within seconds
/// of the test's own wall clock. Choosing a `ValueDate` in the past by hours is
/// what makes "the timestamp did not follow the clock" a claim a wrong
/// implementation cannot satisfy by accident.
const READING_AT: i64 = 1_786_968_000_000;

struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn saboteur(port: u16) -> AsyncClient {
    let mut options = MqttOptions::new("no-replay-saboteur", "127.0.0.1", port);
    options.set_keep_alive(Duration::from_secs(5));
    options.set_max_packet_size(OVERSIZED * 2, OVERSIZED * 2);
    let (client, mut eventloop) = AsyncClient::new(options, 16);
    tokio::spawn(async move { while eventloop.poll().await.is_ok() {} });
    tokio::time::sleep(Duration::from_millis(300)).await;
    client
}

fn a_reading() -> MeterUpdate {
    MeterUpdate::uniform(
        MeterId::new("garage"),
        Measurement {
            meter: MeterId::new("garage"),
            serial: Serial::new(SERIAL),
            power: Some(Kw(0.018)),
            energy: Some(Kwh(4_843.822)),
            value_date: UtcMillis(READING_AT),
            quality: Quality::Good,
        },
        Verdict::good(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reconnect_re_declares_the_reading_without_moving_its_clock() {
    let (_broker, port) = common::start_broker().await;
    let mut seen = common::named_subscriber(port, "no-replay-observer").await;

    let state_dir = std::env::temp_dir().join(format!("chaos_no_replay_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(GROUP, NODE_ID).expect("valid identifiers");
    let (tx, rx) = mpsc::channel(64);
    let (death_tx, death_rx) = oneshot::channel();
    let (_device_tx, device_rx) = mpsc::channel(4);

    // The window the NBIRTH's own timestamp has to fall inside, read from the very
    // clock the driver is about to be handed, before it can have published
    // anything. See the assertion below for why a window and not a value.
    let before_birth = clock.wall().0;

    let driver = tokio::spawn(mqtt_driver::run(
        MqttConfig {
            client_id: NODE_ID.to_string(),
            host: "127.0.0.1".to_string(),
            port,
            keep_alive: Duration::from_secs(30),
            bd_seq_path: state_dir.0.join("bdseq.toml"),
            capacity: 64,
            death_flush: Duration::from_secs(2),
        },
        node,
        vec![Serial::new(SERIAL)],
        Arc::clone(&clock),
        // Story 4.11's counters. Empty on purpose: this test asserts nothing about
        // lost readings.
        smartme_bridge::app::mqtt_driver::Health {
            meters: Heartbeats::default(),
            sink: smartme_bridge::app::mqtt_driver::SinkHealth::new(),
        },
        rx,
        device_rx,
        death_rx,
    ));

    // ---- a session, and one reading in it ---------------------------------
    let nbirth = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await
    .expect("the bridge must birth on its first connection");

    // **The NBIRTH carries the instant it was PUBLISHED, and this is the clause's
    // own words rather than a presence check** —
    // `tck-id-payloads-nbirth-timestamp` (`Sparkplug_6:1064`): *"NBIRTH messages
    // MUST include a payload timestamp that denotes the time at which the message
    // was published"*.
    //
    // **Added by the 2026-08-19 review of story 4.12, which found the row it had
    // moved to `conformant` still satisfied by the mutation [#30] prescribed.**
    // 4.12's `an_hour_of_outage_does_not_move_the_re_declared_reading_forward`
    // hands `birth()` the instant itself, so it proves the publisher stamps the
    // argument it is given — never that the CALL SITE gives it a clock. Replacing
    // `clock.wall()` at `mqtt_driver.rs:1223` and `:1292` with a constant left the
    // whole suite green, which is [#30]'s prescription word for word.
    //
    // A WINDOW, not a value, and the window is what a live clock allows: the
    // bridge is holding a `SystemClock` this test cannot address. It is bounded
    // below by a reading of that same clock taken before the driver existed and
    // above by one taken after the message arrived, so any stamp that is not the
    // publication instant — a constant, the reading's `ValueDate`, a frozen
    // first-connect instant re-used on every rebirth — falls outside it.
    let after_birth = clock.wall().0;
    let born_at = i64::try_from(
        nbirth
            .payload
            .timestamp
            .expect("every Sparkplug payload carries a timestamp"),
    )
    .expect("a Sparkplug timestamp is epoch-millis and fits an i64");
    assert!(
        (before_birth..=after_birth).contains(&born_at),
        "THE NODE BIRTH DOES NOT SPEAK THE PUBLICATION INSTANT. It was published \
         somewhere in [{before_birth}, {after_birth}] and carries {born_at}, so a host \
         pairing this session against its own clock is reading a number the bridge \
         made up (tck-id-payloads-nbirth-timestamp)"
    );
    assert_ne!(
        born_at, READING_AT,
        "the node birth must not carry the READING's clock: a session announcement \
         is not a measurement, and ADR 0013's deviation is deliberately confined to \
         the two rows that re-declare data"
    );

    tx.send(a_reading()).await.expect("the driver is listening");
    let ddata = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/DDATA/")
    })
    .await
    .expect("the reading must reach the wire");

    // The premise, asserted rather than assumed: the DDATA already speaks the
    // reading's clock. If this fails, the reconnect assertion below would be
    // measuring the wrong thing.
    assert_eq!(
        ddata.payload.timestamp,
        Some(READING_AT as u64),
        "the DDATA payload timestamp IS the reading's ValueDate (ADR 0013). \
         Everything below assumes it"
    );

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
    //
    // The DBIRTH, not the NBIRTH: the node birth legitimately carries the
    // reconnection instant, and it is the DEVICE birth that re-declares history.
    let redeclared = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/DBIRTH/")
    })
    .await
    .expect(
        "no device birth arrived within 30 s of the transport breaking. Either the bridge did \
         not reconnect, or it reconnected without re-declaring the device it had already \
         announced",
    );

    let stamped = redeclared
        .payload
        .timestamp
        .expect("every Sparkplug payload carries a timestamp");

    assert_eq!(
        stamped, READING_AT as u64,
        "THE RE-DECLARED READING FOLLOWED THE CLOCK. It was true at {READING_AT} and the \
         rebirth published it stamped {stamped} — so a consumer reading the payload timestamp \
         and ignoring the quality flag sees an outage as a burst of fresh data, which is \
         exactly what FR22's anti-replay clause forbids and what ADR 0013 chose this stamping \
         to prevent"
    );

    // And it is re-declared as history rather than re-asserted as fact.
    assert_eq!(
        redeclared.quality_of(smartme_bridge::adapters::sparkplug_publisher::METRIC_POWER),
        Some(smartme_bridge::adapters::sparkplug_publisher::ignition_quality_code(Quality::Stale)),
        "a reading that has not been re-judged against now must not come back Good"
    );

    // ---- and the session that follows still speaks the reading's clock ----
    //
    // **This replaced a sweep over whatever happened to arrive, which swept ZERO
    // messages** — an assertion over an empty set, which is the hollow shape this
    // repository keeps finding in its own tests. Sending a second reading makes
    // the post-reconnect data path something the test EXERCISES rather than hopes
    // to observe.
    //
    // Its `ValueDate` is a minute after the first and still hours before `now`,
    // so this catches both failures: a bridge stamping the publication instant,
    // and a bridge that simply re-sent the first reading's timestamp.
    const SECOND_READING_AT: i64 = READING_AT + 60_000;
    let mut second = a_reading();
    second.measurement.value_date = UtcMillis(SECOND_READING_AT);
    tx.send(second).await.expect("the driver is listening");

    let after = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/DDATA/")
    })
    .await
    .expect(
        "the bridge must keep publishing after a reconnect — a session that births and then \
         says nothing is the failure story 4.10 was written against",
    );
    assert_eq!(
        after.payload.timestamp,
        Some(SECOND_READING_AT as u64),
        "a reading acquired AFTER the reconnection carries its own acquisition time too. \
         The reconnect must not change which clock the bridge speaks"
    );

    let _ = death_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(10), driver).await;
}
