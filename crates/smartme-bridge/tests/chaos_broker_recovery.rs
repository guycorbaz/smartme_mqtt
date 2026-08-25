//! Story 4.13 AC1–AC3 — the down→up transition, proven from outside the process.
//!
//! # The break is a broker that is GONE, and story 4.12 owns the other one
//!
//! `chaos_no_replay_at_reconnect` (story 4.12) breaks the socket while the broker
//! keeps running: the bridge reconnects on its first attempt, the broker still
//! holds session state, and the observer never loses its subscription. **Here the
//! container is stopped.** Reconnection fails repeatedly against nothing, the
//! backoff ladder actually runs, and every client's session — the observer's
//! included — is destroyed.
//!
//! They are different code paths to the same reconnection, and the second is what
//! a real outage does. **Neither test may be cited as the other's evidence**, and
//! this sentence is written in both files so the boundary is not blurred later by
//! someone counting green tests.
//!
//! # AC2: THE DRAFTING PREMISE WAS WRONG, AND THE MEASUREMENT IS WHY WE KNOW
//!
//! The story was drafted expecting **no death at all**: a will is published by
//! the broker, so stopping the container should destroy the will with the process
//! that held it, and the observer — a client of that same broker — is disconnected
//! in the same instant. Nothing to see, nobody left to see it.
//!
//! **Exactly one NDEATH reaches a subscriber, on every run measured, carrying the
//! live session's own `bdSeq`.** It is the registered will — `qos_for(NDeath)` is
//! `(QoS 1, retain false)`, so this is not a retained message surviving the
//! restart — and mosquitto gets it out to its subscribers before it closes their
//! sockets. `stop_with_timeout(Some(0))` is `docker stop` with a zero grace
//! period, SIGTERM followed immediately by SIGKILL, and mosquitto's SIGTERM path
//! publishes the wills of the sessions it tears down.
//!
//! **A stopped broker is not a crashed one, and that contrast was MEASURED
//! rather than reasoned.** Replacing the stop with `docker kill --signal SIGKILL`
//! and running this same test: `messages observed between broker stop and
//! restart: 0 []`. No death, no anything — there is no shutdown path to run, so
//! the wills die with the process exactly as the story predicted. Nothing here
//! may be read as evidence about a broker that loses power or is killed; this
//! test covers the orderly stop, and only that.
//!
//! # The measurement had a boundary before it had an answer
//!
//! The first version of this counted only what arrived *before* the restart, and
//! measured `0` on 2 runs of 37 while the other 35 measured `1`. Read as it
//! stood, that is "the will sometimes goes missing" — a false property, produced
//! by real runs. It is not the will that moved: it is the instant this test chose
//! to stop looking. The count is now taken on **both** sides of the restart and
//! reported as a total, and the total is 1 on every run of the 17 measured that
//! way. `SETTLE` narrows the boundary; reporting the total is what stops the
//! boundary from being able to lie.
//!
//! **AC2 measures and does not assert**, and that decision is what let the
//! premise be corrected rather than encoded. Had it asserted the absence it would
//! now be red for a true reason; had it asserted the presence it would be a test
//! of mosquitto's shutdown politeness, pinned to a behaviour this project does
//! not own. [#43] asked the same question of the session-takeover path and left
//! it open; this answers it for the outage path, by measurement.
//!
//! # Why the observer is still watching when the broker comes back
//!
//! It is not a hope, it is a race with a known margin. The observer's event loop
//! retries every **100 ms** and re-subscribes on every CONNACK, while the bridge
//! is asleep on a ladder whose floor is **1 s** and which has been doubling for
//! the whole outage — by the restart it is waiting seconds, not milliseconds. The
//! observer is back long before there is anything to miss. The outage is held for
//! [`OUTAGE`] partly to widen that margin.
//!
//! # Falsification — 2026-08-18, four mutations RUN, output copied
//!
//! **AC1, the anti-replay clause.** Moving `Emission::DeviceBirthRedeclaring` to
//! `PublicationInstant` goes red on the re-declaration: `THE RE-DECLARED READING
//! FOLLOWED THE CLOCK … left: 1787081115581, right: 1786968000000` — the observed
//! stamp being the real recovery instant, a day past the reading.
//!
//! **AC1, the session clause.** Removing the `publisher.new_session()` call from
//! the driver's session loop goes red with `THE SESSION NUMBER DID NOT ADVANCE
//! ACROSS THE OUTAGE: born 1, reborn 1`.
//!
//! **AC3, and this is what [#86] was waiting for.** Deleting both `lost(...)`
//! calls from the driver's inbox arm goes red with `NOT ONE DRIVER-SIDE DROP WAS
//! COUNTED … The whole fleet reads: []`. **The same mutation leaves the 258 unit
//! tests passing** — measured, not assumed, and that is precisely the hole [#86]
//! recorded. This test is the first thing in the tree that can see it.
//!
//! **AND HALF OF THAT HOLE IS STILL OPEN, measured by the 2026-08-19 review.** The
//! driver has TWO `lost(...)` call sites, not the four [#86]'s title counts — the
//! title counts REASONS. This test reaches one of them, the
//! `Some(reason) => lost(reason, fault)` arm, through `before-birth`. Deleting the
//! other one on its own — `lost(DropReason::TransportQueueFull, None)`,
//! `mqtt_driver.rs:1408` — leaves this test GREEN, run and observed. So
//! `transport-queue-full` is still counted by nothing that could notice its
//! absence, and `undeclared-device` and `unpublishable` are still pinned by
//! `reason_for`'s mapping alone. Tracked at [#95]; do not read AC3 as covering
//! them.
//!
//! **Task 1, the fixed port.** Swapping `start_broker_on_fixed_port` for
//! `start_broker` goes red on the readiness guard with `the broker did not
//! complete an MQTT CONNACK within 30 s of being restarted` — the restarted
//! container came back on a host port nobody was holding. Note the SHAPE of that failure:
//! without `wait_for_broker` the test would instead have hung to the rebirth
//! deadline, and a hang is the failure this repository keeps having to tell apart
//! from a real defect. The guard is what makes the mutation legible.

mod common;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::mqtt_driver::{self, MqttConfig};
use smartme_bridge::app::poll_publish::{DropReason, Heartbeats};
use smartme_bridge::core::channel::MeterUpdate;
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::core::oracle::Verdict;
use smartme_bridge::domain::{
    DeviceIdentity, Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis,
};

const SERIAL: &str = "30000002";
const NODE_ID: &str = "ChaosRecovery";
const GROUP: &str = "ChaosRecoveryGroup";
const METER: &str = "garage";

/// **2026-08-18T12:00:00Z, and it is deliberately nowhere near `now`.**
///
/// A bridge stamping the publication instant would emit a number within seconds
/// of the test's own wall clock. A `ValueDate` hours in the past is what makes
/// "the timestamp did not follow the clock" a claim a wrong implementation
/// cannot satisfy by accident.
const READING_AT: i64 = 1_786_968_000_000;

/// How long the broker stays down.
///
/// Long enough for two things that are not the same. The driver must re-enter
/// its session loop and drain the inbox with **no birth behind it** — that is
/// what turns the readings below into a `BeforeBirth` count, and it is AC3's
/// whole mechanism. And the reconnect ladder must have climbed well past the
/// observer's 100 ms retry, so the observer is certainly subscribed again before
/// the bridge publishes anything.
const OUTAGE: Duration = Duration::from_secs(8);

/// Time allowed for messages already received to reach the test before the AC2
/// window is closed. See the drain site for what it costs to skip.
const SETTLE: Duration = Duration::from_secs(2);

/// How long the bridge is given to birth once the broker is genuinely back.
///
/// **Sized from the reconnect ladder, not guessed.** `RECONNECT_CEILING` is 30 s
/// and the wait is jittered by up to +50 %, so one step can be 45 s — and the
/// bridge may have just begun such a step when the broker returned. Add one
/// wasted attempt (`rumqttc` times a connection out after 5 s) and the honest
/// bound is around 50 s. **60 s was the first value here and it was too tight**,
/// failing about one run in six. It is 120 s because a chaos test that fails on
/// arithmetic is worse than a slow one — and it only ever costs the full 120 s
/// on a run that was going to fail anyway.
const REBIRTH_DEADLINE: Duration = Duration::from_secs(120);

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
            energy: Some(Kwh(4_843.822)),
            value_date: UtcMillis(value_date),
            quality: Quality::Good,
        },
        Verdict::good(),
    )
}

/// The four reasons the DRIVER owns. `OutboxFull` and `MqttTaskGone` belong to
/// the poll task's send side and are counted elsewhere, so a test that accepted
/// them would pass without the driver's own `lost(...)` calls existing — which
/// is precisely the hole [#86] recorded.
const DRIVER_SIDE: [DropReason; 4] = [
    DropReason::TransportQueueFull,
    DropReason::BeforeBirth,
    DropReason::UndeclaredDevice,
    DropReason::Unpublishable,
];

#[tokio::test(flavor = "multi_thread")]
async fn a_broker_that_comes_back_gets_a_new_session_and_the_old_timestamps() {
    // A port Docker will reopen on restart. `start_broker`'s ephemeral mapping
    // is redrawn by `stop` + `start`, and the bridge — still holding the old
    // number — would reconnect forever to nothing: a test that HANGS rather than
    // fails. See `common::start_broker_on_fixed_port`.
    let (broker, port) = common::start_broker_on_fixed_port().await;
    let mut seen = common::named_subscriber(port, "recovery-observer").await;

    let state_dir = std::env::temp_dir().join(format!("chaos_recovery_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(GROUP, NODE_ID).expect("valid identifiers");
    let (tx, rx) = mpsc::channel(64);
    let (death_tx, death_rx) = oneshot::channel();
    let (_device_tx, device_rx) = mpsc::channel(4);

    // **A REAL fleet, not `Heartbeats::default()`.** The three chaos tests that
    // came before pass `default()`, which is `for_meters([])` — the driver's
    // `dropped` looks the meter up, finds nothing, and returns silently. Every
    // count they might have made went nowhere, and that is the mechanism [#86]
    // names. AC3 is unreachable without this line.
    let pulse = Heartbeats::for_meters([MeterId::new(METER)]);

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
        // Both names since contract v13 (ADR 0049): the topic carries the
        // published one, the declaration is filed under the serial.
        vec![DeviceIdentity::new(
            MeterId::new(METER),
            Serial::new(SERIAL),
        )],
        Arc::clone(&clock),
        smartme_bridge::app::mqtt_driver::Health {
            meters: pulse.clone(),
            sink: smartme_bridge::app::mqtt_driver::SinkHealth::new(),
        },
        rx,
        device_rx,
        death_rx,
    ));

    // ---- a session, and one reading in it ---------------------------------
    let first_birth = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await
    .expect("the bridge must birth on its first connection");
    let first_bd_seq = first_birth
        .bd_seq()
        .expect("an NBIRTH carries bdSeq (tck-id-payloads-nbirth-bdseq)");

    tx.send(a_reading_at(READING_AT))
        .await
        .expect("the driver is listening");
    let ddata = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/DDATA/")
    })
    .await
    .expect("the reading must reach the wire");

    // The premise, ASSERTED rather than assumed: the DDATA already speaks the
    // reading's clock. If this fails, everything below is measuring the wrong
    // thing while looking exactly as green.
    assert_eq!(
        ddata.payload.timestamp,
        Some(READING_AT as u64),
        "the DDATA payload timestamp IS the reading's ValueDate (ADR 0013). \
         Everything below assumes it"
    );

    // ---- the outage --------------------------------------------------------
    broker
        .stop_with_timeout(Some(0))
        .await
        .expect("the broker container stops");

    // Handed to the driver WHILE THE BROKER IS DOWN. The driver's session ended
    // with the transport; it re-enters the session loop, rebuilds a client that
    // will never reach CONNACK, and drains these with the publisher back in
    // `Session::Pending` — so each one returns `DroppedBeforeBirth` and reaches
    // `lost(...)`. That is what AC3 counts.
    //
    // **HOW AC3 COULD GO RED WITHOUT A DEFECT IN THE BRIDGE**, because arguing a
    // mechanism's robustness without naming its window is the habit this
    // repository keeps having to break. A reading that reaches the inbox arm
    // BEFORE the driver has noticed the transport is gone is still published into
    // a live-looking session: `try_publish` answers `Ok` on entering `rumqttc`'s
    // request channel, the message never leaves the socket, and it is discarded
    // with the event loop `pump.abort()` kills — **uncounted**, because
    // `reason_for` said `None` and `publish_all` reported no failure. That is
    // [#85], already open and already recorded at the call site
    // (`mqtt_driver.rs`, `reason_for`'s doc comment). If all three readings
    // landed in that window, no counter would move and the AC3 assertion below
    // would fail against a bridge doing exactly what it is documented to do.
    //
    // It is left as it is rather than papered over with a retry, and the reason
    // is that the failure is the SAFE one: [#85] can only make this test red,
    // never green, so no property is silently unproven. The window is also very
    // narrow — `stop_with_timeout` returns once the container is actually
    // stopped, so the socket is already dead when these are sent. Measured
    // `("garage", "before-birth", 3)` on every run so far. **If this assertion
    // ever fails on a green-looking bridge, read [#85] before reading anything
    // else.**
    for n in 1..=3 {
        tx.send(a_reading_at(READING_AT + n * 1_000))
            .await
            .expect("the driver is listening even with no broker");
    }

    tokio::time::sleep(OUTAGE).await;

    // ---- AC2: the measurement, not an assertion ----------------------------
    //
    // Everything the observer received between the stop and the restart. The
    // expectation is NONE, for the reason in the module header, and the count is
    // printed either way: a test that swept an empty set and called it coverage
    // is the exact defect story 4.12's first draft shipped.
    //
    // **The drain settles first, and that is not politeness.** Draining straight
    // out of the sleep measured `0` on one run in twenty-three while the other
    // twenty-two measured `1` — not because the broker behaved differently, but
    // because the observer task had not yet forwarded a message it had already
    // received. The boundary of a measurement has to be real, or the measurement
    // is of the harness. `chaos_ncmd_subscription` learned the same lesson and
    // calls it a settle.
    tokio::time::sleep(SETTLE).await;
    let mut during_outage = Vec::new();
    while let Ok(message) = seen.try_recv() {
        during_outage.push((message.topic.clone(), message.bd_seq()));
    }
    println!(
        "AC2 MEASUREMENT — messages observed between broker stop and restart: {} {:?}",
        during_outage.len(),
        during_outage
    );
    let deaths: Vec<_> = during_outage
        .iter()
        .filter(|(topic, _)| topic.contains("/NDEATH/") || topic.contains("/DDEATH/"))
        .collect();
    // The bdSeq is printed with each death because a death is only meaningful
    // against the session it covers: one carrying `first_bd_seq` is the will the
    // broker registered at CONNECT, fired on the way down. A death carrying any
    // other number would be a different — and much worse — story.
    println!(
        "AC2 MEASUREMENT — of those, deaths: {} {:?} (the live session was bdSeq {})",
        deaths.len(),
        deaths,
        first_bd_seq
    );

    // ---- the broker comes back ---------------------------------------------
    broker.start().await.expect("the broker container restarts");
    // `ContainerAsync::start` does NOT re-apply the image's `WaitFor` — it issues
    // the Docker start and returns. Without this the test would race mosquitto's
    // own boot, and losing that race is indistinguishable from a bridge that
    // never reconnected.
    assert!(
        common::wait_for_broker(port, Duration::from_secs(30)).await,
        "the broker did not complete an MQTT CONNACK within 30 s of being restarted; \
         nothing below would be about the bridge"
    );

    // ---- AC1: a NEW session number -----------------------------------------
    // **The AC2 window has two sides, and only measuring one of them produced a
    // false property.** Draining before the restart measured `0` on 2 runs of 37
    // while the other 35 measured `1` — which reads as "the will sometimes goes
    // missing" until you look at the other side of the boundary and find it
    // there. Everything the rebirth wait passes over is recorded here, so the
    // record can state how many deaths reached a subscriber IN TOTAL rather than
    // how many happened to land before an instant the test chose.
    let after_restart = std::cell::RefCell::new(Vec::new());
    let reborn = common::wait_for(&mut seen, REBIRTH_DEADLINE, |s| {
        after_restart.borrow_mut().push(s.topic.clone());
        s.topic.contains("/NBIRTH/")
    })
    .await
    .expect(
        "no NBIRTH arrived within the rebirth deadline of the broker coming back. \
         Either the bridge stopped \
         reconnecting, or it reconnected without birthing — a session no consumer can trust",
    );
    let late_deaths: Vec<String> = after_restart
        .borrow()
        .iter()
        .filter(|t| t.contains("/NDEATH/") || t.contains("/DDEATH/"))
        .cloned()
        .collect();
    println!(
        "AC2 MEASUREMENT — deaths seen AFTER the restart, before the rebirth: {} {:?}",
        late_deaths.len(),
        late_deaths
    );
    println!(
        "AC2 MEASUREMENT — deaths reaching a subscriber IN TOTAL across the outage: {}",
        deaths.len() + late_deaths.len()
    );

    let reborn_bd_seq = reborn
        .bd_seq()
        .expect("an NBIRTH carries bdSeq (tck-id-payloads-nbirth-bdseq)");
    assert!(
        reborn_bd_seq > first_bd_seq,
        "THE SESSION NUMBER DID NOT ADVANCE ACROSS THE OUTAGE: born {first_bd_seq}, reborn \
         {reborn_bd_seq}. A consumer pairing death to birth by bdSeq cannot tell the new \
         session from the one the broker's will already covered"
    );

    // ---- AC1: and the re-declared reading keeps ITS OWN clock ---------------
    //
    // The DBIRTH, not the NBIRTH: the node birth legitimately carries the
    // reconnection instant, and it is the DEVICE birth that re-declares history.
    let redeclared = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/DBIRTH/")
    })
    .await
    .expect(
        "the node was reborn but the device was not re-declared, so a host that had the device \
         on screen keeps a value nothing will ever correct",
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
         exactly what FR22's anti-replay clause forbids"
    );
    assert_eq!(
        redeclared.quality_of(smartme_bridge::adapters::sparkplug_publisher::METRIC_POWER),
        Some(smartme_bridge::adapters::sparkplug_publisher::ignition_quality_code(Quality::Stale)),
        "a reading that has not been re-judged against now must not come back Good"
    );

    // ---- AC1: and the session that follows still speaks the reading's clock -
    //
    // Its `ValueDate` is a minute after the first and still hours before `now`,
    // so this catches BOTH failures: a bridge stamping the publication instant,
    // and a bridge frozen on the timestamp it re-declared.
    const AFTER_RECOVERY_AT: i64 = READING_AT + 60_000;
    tx.send(a_reading_at(AFTER_RECOVERY_AT))
        .await
        .expect("the driver is listening");
    let after = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/DDATA/")
    })
    .await
    .expect(
        "the bridge must keep publishing after the broker comes back — a session that births \
         and then says nothing is the failure story 4.10 was written against",
    );
    assert_eq!(
        after.payload.timestamp,
        Some(AFTER_RECOVERY_AT as u64),
        "a reading acquired AFTER the recovery carries its own acquisition time too. \
         An outage must not change which clock the bridge speaks"
    );

    // ---- AC3: the counters moved, and [#86] closes on this ------------------
    let fleet = pulse.snapshot();
    let moved: Vec<_> = fleet
        .dropped()
        .into_iter()
        .filter(|lost| DRIVER_SIDE.contains(&lost.reason))
        .map(|lost| (lost.meter.to_string(), lost.reason.as_str(), lost.count))
        .collect();
    println!("AC3 MEASUREMENT — driver-side drop counters after the outage: {moved:?}");
    assert!(
        !moved.is_empty(),
        "NOT ONE DRIVER-SIDE DROP WAS COUNTED, though three readings were handed over with no \
         broker to send them to. The driver's two `lost(...)` calls are the only thing that \
         moves these cells, and until this test existed deleting BOTH of them left the whole \
         suite green ([#86]). Deleting only the `transport-queue-full` one still does ([#95]). \
         The whole fleet reads: {:?}",
        fleet
            .dropped()
            .into_iter()
            .map(|lost| (lost.meter.to_string(), lost.reason.as_str(), lost.count))
            .collect::<Vec<_>>()
    );
    // WHICH reason is deliberately not asserted: whether the outage produces
    // `before-birth`, `transport-queue-full`, or both depends on where the
    // reconnect ladder happened to be when each reading arrived — timing this
    // test does not control. Pinning it would pin the harness, not the property.
    //
    // **The cost of that choice, named rather than left to be rediscovered:** every
    // run measured so far lands on `before-birth`, so the OTHER call site — the
    // `transport-queue-full` one at `mqtt_driver.rs:1408` — is never exercised here.
    // Deleting it alone leaves this assertion green (measured, 2026-08-19). [#95].

    let _ = death_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(10), driver).await;
}
