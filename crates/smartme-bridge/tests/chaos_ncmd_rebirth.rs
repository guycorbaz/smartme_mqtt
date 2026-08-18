//! Story 4.7 — a real `Node Control/Rebirth` request, answered on the wire.
//!
//! Five properties, every one of them asserted from an INDEPENDENT subscriber's
//! transcript rather than from anything the bridge says about itself:
//!
//! 1. **Every NBIRTH declares the command** (AC1). The metric a host addresses
//!    has to be visible to a host, so it is read off the decoded payload a third
//!    party received — not off the builder.
//! 2. **A request is answered with a COMPLETE birth sequence** (AC2):
//!    `tck-id-operational-behavior-data-commands-rebirth-action-2` says *"a
//!    complete BIRTH sequence including the NBIRTH and DBIRTH(s)"*, with the
//!    NBIRTH restarting the numbering at 0 and the DBIRTHs continuing it.
//! 3. **No DATA crosses the answer** (AC3): `-rebirth-action-1` requires the node
//!    to *"immediately stop sending DATA messages"*.
//! 4. **And DATA RESUMES afterwards** (AC3's second half). Added by the Story 4.7
//!    code review: only the absence was asserted, so a driver that stopped
//!    publishing DATA PERMANENTLY after answering a rebirth — a total, silent loss
//!    of the bridge's purpose, triggerable by any client on an unauthenticated
//!    broker — satisfied every assertion in this file.
//! 5. **The session is not re-opened** (AC5): `-rebirth-action-3` requires the
//!    same `bdSeq` as the will registered at CONNECT, asserted THROUGH THE NCMD
//!    PATH rather than through the reconnect path that Story 4.6 already covers.
//!
//! # Why this test drives the driver in-process, and the others spawn the binary
//!
//! Because AC3 is otherwise VACUOUS, and it would have been vacuous in exactly
//! the way this project has already thrown four tests away for.
//!
//! Every other chaos test spawns the real binary with `SMARTME_API_BASE` pointed
//! at TEST-NET-1, which is unroutable — so the cloud never answers, the poll task
//! never produces a reading, and **no DDATA is ever published at all**. An
//! assertion that no DATA interleaves the birth sequence would then hold on a
//! stream that contains no DATA anywhere, and it would keep holding against a
//! bridge that deferred the answer behind a flag, spawned it, or queued it. It
//! would be green forever and would mean nothing.
//!
//! So this test owns the inbox instead: it feeds `mqtt_driver::run` a stream of
//! judged readings and asserts, BEFORE the request is sent, that DATA was actually
//! flowing — so that if the stream ever dries up the test fails as a broken premise
//! rather than passing as a satisfied criterion.
//!
//! **How the window is made real, corrected by the Story 4.7 code review.** This
//! used to claim the 20 ms cadence was *"fast enough that the driver always has a
//! DATA message waiting"*. It is the other way round: 20 ms is SLOW relative to the
//! driver's per-message work, so the driver handles each reading in microseconds
//! and then parks in `select!` with an empty inbox. The window was therefore ~0
//! messages wide, and the deferral mutations it exists to catch passed whenever
//! they completed inside one tick.
//!
//! The feeder is now STOPPED before the request, and the stream is allowed to go
//! quiet — which removes a genuine flake, because the window's left edge is the
//! OBSERVER's receipt of the NCMD and the NCMD arrives over a different connection
//! from the bridge's DDATA, so a legitimately-published reading could land inside
//! the window and fail the test with a message blaming the driver. A single reading
//! is then pushed 50 ms AFTER the request, which is what a flag-based deferral
//! needs in order to fire, and what proves DATA resumes.
//!
//! The oracle is still external: the broker is a real container and the
//! transcript comes from a subscriber with no relationship to the bridge beyond
//! it. What is in-process is the *stimulus*, not the *observation*.
//!
//! # Every way this test could pass for the wrong reason
//!
//! - *The window is empty because nothing was pending.* The subtler form of the
//!   trap below, and the one this file was in until the Story 4.7 code review: DATA
//!   had flowed, so the premise check passed, but nothing was PENDING when the
//!   command was dequeued, so the window contained no DATA under any
//!   implementation. Excluded by pushing a reading 50 ms after the request.
//! - *The window fails because a legitimate DDATA raced the NCMD.* Two publishers,
//!   two connections: only the bridge's messages are ordered relative to each
//!   other, so a reading published before the command was dequeued could be
//!   delivered after the NCMD was. Excluded by stopping the feeder and waiting for
//!   the stream to go quiet before the request.
//! - *No DATA is flowing, so AC3's window is empty.* The exact trap above.
//!   Excluded by requiring several DDATA messages in the transcript before the
//!   request is published, with its own failure message.
//! - *The "second" NBIRTH is a reconnect, not an answer.* A reconnect would also
//!   produce an NBIRTH under the same `bdSeq`, so `bdSeq` alone cannot tell them
//!   apart. Excluded by requiring the second NBIRTH to appear AFTER the NCMD in
//!   the transcript, and by the run being short enough and the broker local
//!   enough that no reconnect is provoked; a reconnect would additionally show a
//!   gap in the DDATA stream, which the DATA-continuity check would report.
//! - *The observer misses the NCMD, so the AC3 window starts too early.* It
//!   cannot: the observer subscribes to `spBv1.0/#`, which includes the NCMD
//!   topic, and the payload is a well-formed Sparkplug payload so it survives
//!   the decode-or-discard in `common::named_subscriber_on`.
//! - *The request is published before the bridge has subscribed.* Excluded by
//!   waiting for the first NBIRTH, which the bridge publishes only AFTER its
//!   SUBSCRIBE on the same connection (Story 4.6 asserts that ordering from the
//!   broker's own log).
//! - *The bdSeq assertion compares a constant with itself.* It does not: the
//!   first value is read from the FIRST NBIRTH and the second from the answer,
//!   both decoded from the transcript. This is the shape of the Epic 1 `bdSeq`
//!   tautology and it is why both sides are read rather than one.
//!
//! # Falsification — run against deliberately broken code
//!
//! Quotes are copied from the run's own output, never reconstructed. The Story 4.7
//! code review found this table disagreeing with the story's copy on two entries
//! and quoting a failure message that appears nowhere in this file — and a
//! falsification record whose quote was written from memory is indistinguishable
//! from one that was never run.
//!
//! | Mutation | Result |
//! | --- | --- |
//! | the `Inbound::Rebirth` arm of the command branch deleted (the command is classified and traced, never acted on) | RED — *"no second NBIRTH followed the Rebirth Request"* |
//! | `announce` called with `BirthReason::RebirthRequested` but preceded by `publisher.new_session()` | RED — *"the answer opened a NEW session"*, `left: Some(2)` / `right: Some(1)`, i.e. bdSeq **1 → 2**. Re-run 2026-07-31 by the code review: this table said `0 → 1` and the story's said `1 → 2`, and only one experiment was ever performed |
//! | the rebirth answer deferred behind a flag consumed one message later | RED — *"1 DATA message(s) were published inside the birth sequence"* |
//! | `classify` matching on the metric NAME alone | GREEN — and correctly so: a name-and-value request still matches. The name-only case is covered by the unit tests, which do go red. |
//! | the DDATA feeder stopped (premise mutation) | RED — *"no DATA was flowing before the request"*, which is the anti-vacuity guard reporting itself rather than the AC passing quietly |
//! | the rebirth metric removed from the `Session::Live` arm only | RED — *"the ANSWERING NBIRTH declares no Node Control/Rebirth metric"*. Added by the review: this file checked only the FIRST birth, and `-nbirth-rebirth-req` binds *"Every NBIRTH"* |
//! | DATA publication stopped permanently after the answer | RED — *"no DATA was published after the birth sequence completed"*. Added by the review; before it, this mutation left the file green |

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::mqtt_driver::{self, MqttConfig};
use smartme_bridge::core::channel::MeterUpdate;
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};

use common::Seen;

const SERIAL: &str = "30000001";
const NODE_ID: &str = "ChaosRebirth";
const GROUP: &str = "ChaosRebirthGroup";
const REBIRTH_METRIC: &str = "Node Control/Rebirth";

fn ncmd_topic() -> String {
    format!("spBv1.0/{GROUP}/NCMD/{NODE_ID}")
}

/// Removes the state directory on every path, including the failing ones.
struct ScratchDir(std::path::PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A client that only ever sends commands at the bridge.
///
/// It waits for its own CONNACK before returning: a command published before the
/// broker accepted the connection is dropped by `rumqttc`'s queue, and the test
/// would then blame the bridge for never seeing it.
async fn commander(port: u16) -> AsyncClient {
    let mut options = MqttOptions::new("rebirth-commander", "127.0.0.1", port);
    options.set_keep_alive(Duration::from_secs(5));
    let (client, mut eventloop) = AsyncClient::new(options, 32);
    let (ready_tx, ready_rx) = oneshot::channel();
    tokio::spawn(async move {
        let mut ready_tx = Some(ready_tx);
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    if let Some(tx) = ready_tx.take() {
                        let _ = tx.send(());
                    }
                }
                Ok(_) => {}
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    });
    ready_rx.await.expect("the commander connected");
    client
}

/// A conformant Rebirth Request: the name AND the boolean value `true`, as
/// `tck-id-operational-behavior-data-commands-ncmd-rebirth-name` and
/// `-ncmd-rebirth-value` define it together.
///
/// The value is spelled out rather than left to `..Default::default()`, which
/// would produce `value: None` — a payload this bridge deliberately does NOT
/// answer, and one that no conformant host sends.
fn rebirth_request() -> Vec<u8> {
    sparkplug_b::encode(&sparkplug_b::protobuf::Payload {
        timestamp: Some(1_700_000_000_000),
        metrics: vec![sparkplug_b::protobuf::payload::Metric {
            name: Some(REBIRTH_METRIC.to_string()),
            datatype: Some(sparkplug_b::DataType::Boolean.code()),
            value: Some(sparkplug_b::protobuf::payload::metric::Value::BooleanValue(
                true,
            )),
            ..Default::default()
        }],
        seq: None,
        uuid: None,
        body: None,
    })
}

/// One judged reading, as the poll task would hand it over.
fn reading(now: UtcMillis) -> MeterUpdate {
    MeterUpdate::uniform(
        MeterId::new("garage"),
        Measurement {
            meter: MeterId::new("garage"),
            serial: Serial::new(SERIAL),
            power: Some(Kw(0.018)),
            energy: Some(Kwh(4_843.822)),
            value_date: now,
            quality: Quality::Good,
        },
        smartme_bridge::core::oracle::Verdict::good(),
    )
}

/// The transcript, in the order a third party received it.
type Transcript = Arc<Mutex<Vec<Seen>>>;

/// Drains the observer into a shared transcript.
///
/// A collector rather than repeated `wait_for` calls: AC3 is a statement about
/// what did NOT appear between two messages, so the ordering of everything in
/// between is the evidence — and a receiver that is only polled when the test
/// asks a question loses exactly that.
fn collect(mut rx: mpsc::Receiver<Seen>) -> Transcript {
    let transcript: Transcript = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&transcript);
    tokio::spawn(async move {
        while let Some(seen) = rx.recv().await {
            sink.lock().expect("not poisoned").push(seen);
        }
    });
    transcript
}

/// Waits until `predicate` holds over the transcript, or gives up.
async fn until(
    transcript: &Transcript,
    timeout: Duration,
    predicate: impl Fn(&[Seen]) -> bool,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if predicate(&transcript.lock().expect("not poisoned")) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

fn is(seen: &Seen, kind: &str) -> bool {
    seen.topic.contains(&format!("/{kind}/"))
}

fn count(seen: &[Seen], kind: &str) -> usize {
    seen.iter().filter(|s| is(s, kind)).count()
}

/// Renders the transcript for a failure message: without it, every assertion
/// below fails with a bare boolean and the next reader has to re-run the test to
/// learn anything.
fn render(seen: &[Seen]) -> String {
    seen.iter()
        .enumerate()
        .map(|(i, s)| format!("  {i:>3}  seq={:?}  {}", s.payload.seq, s.topic))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn chaos_a_rebirth_request_is_answered_with_a_complete_birth_sequence() {
    let (_broker, port) = common::start_broker().await;
    let transcript = collect(common::named_subscriber(port, "rebirth-observer").await);

    let state_dir = std::env::temp_dir().join(format!("chaos_rebirth_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(GROUP, NODE_ID).expect("valid identifiers");
    let (tx, rx) = mpsc::channel(64);
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
        },
        node,
        vec![Serial::new(SERIAL)],
        Arc::clone(&clock),
        // Story 4.11's drop counters. EMPTY ON PURPOSE: this test asserts nothing
        // about lost readings, and `Heartbeats::dropped` skips a meter it does not
        // serve rather than panicking — so an empty fleet here counts nothing and
        // changes nothing. A test that wants the counts must build one for its own
        // meters.
        smartme_bridge::app::poll_publish::Heartbeats::default(),
        rx,
        // AC4's reconfiguration channel. These tests never send on it; the
        // sender is kept alive so the driver's branch stays armed rather than
        // disarming on a dropped end.
        device_rx,
        death_rx,
    ));

    // The DATA stream whose EXISTENCE makes AC3's window non-vacuous.
    //
    // Every other chaos test points the binary at TEST-NET-1, so no reading is ever
    // fetched and no DDATA is ever published — an assertion that no DATA interleaves
    // a birth sequence would hold over an empty stream, green forever, and green
    // against every mutation that breaks the clause. This test drives
    // `mqtt_driver::run` in process and feeds it real readings so that there is
    // something for the clause to be about.
    //
    // **What this cadence does NOT establish**, corrected by the Story 4.7 code
    // review, which found the claim here inverted. 20 ms is *slow* relative to the
    // driver's per-message work, not fast: the driver handles each reading in
    // microseconds and then parks in `select!`, so the `inbox` branch is almost
    // always EMPTY rather than always ready. The window is therefore made real by
    // the pending reading pushed after the request (see below), not by this
    // cadence. This feeder's job is the premise — that DATA was flowing at all.
    let feeder = {
        let clock = Arc::clone(&clock);
        let tx = tx.clone();
        tokio::spawn(async move {
            loop {
                if tx.send(reading(clock.wall())).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
    };

    // ---------------------------------------------------------------- AC1 ---
    assert!(
        until(&transcript, Duration::from_secs(30), |seen| {
            count(seen, "NBIRTH") >= 1
        })
        .await,
        "no NBIRTH reached the observer, so nothing below can be judged.\n{}",
        render(&transcript.lock().expect("not poisoned"))
    );

    let first_birth = {
        let seen = transcript.lock().expect("not poisoned");
        seen.iter()
            .find(|s| is(s, "NBIRTH"))
            .cloned()
            .expect("just asserted present")
    };

    let declared = first_birth
        .payload
        .metrics
        .iter()
        .find(|m| m.name.as_deref() == Some(REBIRTH_METRIC))
        .unwrap_or_else(|| {
            let names: Vec<&str> = first_birth
                .payload
                .metrics
                .iter()
                .filter_map(|m| m.name.as_deref())
                .collect();
            panic!(
                "the NBIRTH a third party received declares no {REBIRTH_METRIC} metric; \
                 it carries {names:?}. tck-id-topics-nbirth-rebirth-metric, \
                 tck-id-payloads-nbirth-rebirth-req and \
                 tck-id-operational-behavior-data-commands-rebirth-name are all MUSTs, \
                 and without the metric a Host Application has no declared endpoint to \
                 address — the handler below is unreachable by a conformant host."
            )
        });
    assert_eq!(
        declared.datatype,
        Some(sparkplug_b::DataType::Boolean.code()),
        "-rebirth-datatype is a MUST on Boolean (code 11)"
    );
    assert_eq!(
        declared.value,
        Some(sparkplug_b::protobuf::payload::metric::Value::BooleanValue(
            false
        )),
        "-rebirth-value is a MUST on the value false, and a null metric is not it"
    );
    assert_eq!(
        declared.alias, None,
        "-rebirth-name-aliases forbids an alias on this metric so that a host can \
         request a rebirth without knowing one"
    );

    let bd_seq_at_birth = first_birth.bd_seq().expect("the NBIRTH carries a bdSeq");

    // ------------------------------------------------ AC3's premise, first ---
    // Assert the window EXISTS before asserting what is not in it. Without this,
    // a bridge that publishes no DATA at all satisfies AC3 trivially — which is
    // precisely how a test passes for a reason unrelated to its own name.
    assert!(
        until(&transcript, Duration::from_secs(30), |seen| {
            count(seen, "DDATA") >= 3
        })
        .await,
        "no DATA was flowing before the Rebirth Request, so the AC3 assertion below \
         would hold over an empty window and prove nothing. This is a broken \
         premise, not a satisfied criterion.\n{}",
        render(&transcript.lock().expect("not poisoned"))
    );

    // ------------------------------------- and then STOP it, deliberately ---
    //
    // The Story 4.7 code review found this window could fail for a reason that is
    // not a defect. Its left edge is the OBSERVER's receipt of the NCMD, and the
    // NCMD travels commander → broker → observer while a DDATA travels
    // bridge → broker → observer. Two publishers, two connections: a DDATA the
    // bridge published entirely legitimately can be delivered after the NCMD and
    // land inside the window, failing the assertion with a message that accuses
    // the driver of violating `-rebirth-action-1`. With a reading every 20 ms that
    // is a live flake, not a theoretical one.
    //
    // So the feeder is stopped and the stream is allowed to go quiet BEFORE the
    // request is sent. Nothing of the bridge's is then in flight, so a DATA message
    // inside the window can only have been published after the request arrived —
    // which is the property, with the coincidence removed rather than tolerated.
    feeder.abort();
    let quiet_from = {
        // Quiescence, measured rather than assumed: wait until the DDATA count
        // stops changing. A fixed sleep would be a guess about scheduling.
        let mut last = count(&transcript.lock().expect("not poisoned"), "DDATA");
        loop {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let now = count(&transcript.lock().expect("not poisoned"), "DDATA");
            if now == last {
                break now;
            }
            last = now;
        }
    };
    assert!(
        quiet_from >= 3,
        "the stream must have FLOWED and then stopped; {quiet_from} DDATA is not a \
         flow that was interrupted"
    );

    let before = transcript.lock().expect("not poisoned").len();

    // ---------------------------------------------------------- AC2, 3, 5 ---
    let commander = commander(port).await;
    commander
        .publish(ncmd_topic(), QoS::AtLeastOnce, false, rebirth_request())
        .await
        .expect("the Rebirth Request is queued");

    // One reading, pushed shortly AFTER the request, and it does two jobs.
    //
    // **It gives the deferral mutations something to trip over.** A driver that
    // consumes a deferred answer "on the next message" publishes THIS reading's
    // DDATA before the birth it owes, which puts a DATA inside the window below.
    // Without a pending reading the window is empty under every implementation
    // whose answer completes before the next tick, which is what the review found:
    // the recorded RED for that mutation was probabilistic, not structural.
    //
    // **It also proves DATA RESUMES.** AC3 says DATA stops and does not resume
    // *until the sequence is out* — and nothing asserted the second half, so a
    // driver that stopped publishing DATA permanently after answering a rebirth
    // (the most damaging silent regression this change could have) satisfied every
    // assertion in the file.
    //
    // The delay is 50 ms so that a CORRECT driver has certainly dequeued and
    // answered first. What that buys and what it does not: a flag-based deferral is
    // caught, because it needs this very message to fire; a `tokio::spawn` deferral
    // that completes inside 50 ms is NOT. That limit is stated rather than papered
    // over — `CLAUDE.md` forbids claiming an AC on a mutation that cannot be made
    // to fail, and the spawn shape is caught instead by the inline-and-synchronous
    // comment at the call site being load-bearing, plus review.
    let resume_probe = {
        let clock = Arc::clone(&clock);
        let tx = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = tx.send(reading(clock.wall())).await;
        })
    };

    // The answer is one NBIRTH followed by one DBIRTH (one configured meter).
    let answered = until(&transcript, Duration::from_secs(30), |seen| {
        let tail = &seen[before.min(seen.len())..];
        let Some(birth_at) = tail.iter().position(|s| is(s, "NBIRTH")) else {
            return false;
        };
        tail[birth_at..].iter().any(|s| is(s, "DBIRTH"))
    })
    .await;

    let seen = transcript.lock().expect("not poisoned").clone();
    assert!(
        answered,
        "no second NBIRTH followed the Rebirth Request. \
         tck-id-operational-behavior-data-commands-rebirth-action-2 requires a \
         COMPLETE birth sequence — NBIRTH and DBIRTH(s) — on receipt, and a request \
         that is recognised, traced and then dropped repairs exactly as much as one \
         that never arrived.\n{}",
        render(&seen)
    );

    let tail = &seen[before.min(seen.len())..];
    let command_at = tail.iter().position(|s| is(s, "NCMD")).unwrap_or_else(|| {
        panic!(
            "the observer never saw the NCMD it is about to reason about, so the AC3 \
             window has no left-hand end.\n{}",
            render(&seen)
        )
    });
    let birth_at = tail[command_at..]
        .iter()
        .position(|s| is(s, "NBIRTH"))
        .map(|i| command_at + i)
        .unwrap_or_else(|| {
            panic!(
                "the second NBIRTH does not follow the NCMD in the transcript, so it \
             cannot be attributed to the request — it would be a reconnect.\n{}",
                render(&seen)
            )
        });
    let last_dbirth_at = tail
        .iter()
        .enumerate()
        .skip(birth_at)
        .filter(|(_, s)| is(s, "DBIRTH"))
        .map(|(i, _)| i)
        .next_back()
        .expect("the answer's DBIRTH, just asserted present");

    // AC2 — the sequence restarts and continues.
    assert_eq!(
        tail[birth_at].payload.seq,
        Some(0),
        "the answering NBIRTH must restart the numbering at 0: a consumer reads a \
         non-zero seq on an NBIRTH as a gap in a stream it has not seen.\n{}",
        render(&seen)
    );
    assert_eq!(
        tail[last_dbirth_at].payload.seq,
        Some(1),
        "the DBIRTH continues the edge node's numbering from the NBIRTH; the sequence \
         is per EDGE NODE and shared by node and device messages.\n{}",
        render(&seen)
    );

    // AC5 — a rebirth re-announces a session, it does not open one.
    assert_eq!(
        tail[birth_at].bd_seq(),
        Some(bd_seq_at_birth),
        "the answer opened a NEW session. \
         tck-id-operational-behavior-data-commands-rebirth-action-3: *\"The NBIRTH \
         MUST include the same bdSeq metric with the same value it had included in \
         the Will Message of the previous MQTT CONNECT packet\"* — because no new \
         MQTT session is being established. Advancing it leaves the broker holding a \
         will for a session number the live node no longer claims, so the death that \
         eventually fires is discarded by any consumer that pairs death to birth by \
         bdSeq: the node dies and its tags stay green. Both values are read from the \
         transcript, never one from a constant.\n{}",
        render(&seen)
    );

    // AC1 — on the ANSWERING NBIRTH, not only on the first one.
    //
    // `tck-id-payloads-nbirth-rebirth-req` binds *"Every NBIRTH"*. The AC1 block
    // above reads `first_birth`, which is produced by the `Session::Pending` arm;
    // the arm that runs on every reconnect and every rebirth answer is
    // `Session::Live`, and until the Story 4.7 code review this file checked that
    // one for `seq` and `bdSeq` only. The unit test covers both arms, but the
    // end-to-end evidence for the clause was narrower than the clause.
    let answering = tail[birth_at]
        .payload
        .metrics
        .iter()
        .find(|m| m.name.as_deref() == Some(REBIRTH_METRIC))
        .unwrap_or_else(|| {
            let present: Vec<&str> = tail[birth_at]
                .payload
                .metrics
                .iter()
                .filter_map(|m| m.name.as_deref())
                .collect();
            panic!(
                "the ANSWERING NBIRTH declares no {REBIRTH_METRIC} metric; it carries \
                 {present:?}. tck-id-payloads-nbirth-rebirth-req binds EVERY NBIRTH, \
                 and this is the arm that runs on every reconnect and every rebirth \
                 — a metric present only on the first birth is not conformance.\n{}",
                render(&seen)
            )
        });
    assert_eq!(
        answering.datatype,
        Some(sparkplug_b::DataType::Boolean.code()),
        "the answering NBIRTH's rebirth metric must still be Boolean (code 11)\n{}",
        render(&seen)
    );
    assert_eq!(
        answering.value,
        Some(sparkplug_b::protobuf::payload::metric::Value::BooleanValue(
            false
        )),
        "-rebirth-value is a MUST on the value false, on every NBIRTH\n{}",
        render(&seen)
    );
    assert_eq!(
        answering.alias,
        None,
        "-rebirth-name-aliases forbids an alias here, on every NBIRTH\n{}",
        render(&seen)
    );

    // AC3 — nothing that carries data crosses the answer.
    let interleaved: Vec<&Seen> = tail[command_at..=last_dbirth_at]
        .iter()
        .filter(|s| is(s, "DDATA") || is(s, "NDATA"))
        .collect();
    assert!(
        interleaved.is_empty(),
        "{} DATA message(s) were published inside the birth sequence. \
         tck-id-operational-behavior-data-commands-rebirth-action-1: *\"When an Edge \
         Node receives a Rebirth Request, it MUST immediately stop sending DATA \
         messages\"*. The bridge satisfies this by SHAPE — one `select!` branch runs \
         to completion — so anything that defers the answer (a spawn, a flag consumed \
         later, a channel) breaks the clause with nothing else to notice it. A DATA \
         stream WAS flowing before the request and was then stopped, and a reading \
         was pushed 50 ms after the request, so this window is neither empty nor \
         raced by an in-flight publication.\n{}",
        interleaved.len(),
        render(&seen)
    );

    // AC3, second half — and DATA RESUMES.
    //
    // The clause is *"stop sending DATA"*, and the AC is written as "does not resume
    // until the sequence is out". Only the absence was ever asserted, so a driver
    // that stopped publishing DATA PERMANENTLY after answering a rebirth passed
    // every assertion in this file — a silent, total loss of the bridge's purpose,
    // triggerable by any client on an unauthenticated broker. Found by the Story 4.7
    // code review.
    resume_probe.await.expect("the resume probe ran");
    let resumed = until(&transcript, Duration::from_secs(30), |seen| {
        let tail = &seen[before.min(seen.len())..];
        tail.iter().skip(last_dbirth_at + 1).any(|s| is(s, "DDATA"))
    })
    .await;
    assert!(
        resumed,
        "no DATA was published after the birth sequence completed. Stopping DATA on \
         receipt is half of -rebirth-action-1; a node that stops and never restarts \
         satisfies the letter of the clause and none of its purpose, and nothing in \
         this file would have noticed.\n{}",
        render(&transcript.lock().expect("not poisoned"))
    );

    driver.abort();
}
