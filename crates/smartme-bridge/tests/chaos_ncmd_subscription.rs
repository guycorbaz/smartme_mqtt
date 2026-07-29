//! Story 4.6 — the command subscription exists, is answered, and every command
//! is thrown away safely.
//!
//! Three properties, and each is asserted against a source OUTSIDE the bridge
//! wherever one exists:
//!
//! 1. **The SUBSCRIBE reaches the broker before the NBIRTH.** Asserted from the
//!    broker's own verbose log, not from the bridge's. `tck-id-message-flow-
//!    edge-node-ncmd-subscribe`'s preamble (`Sparkplug_5_Operational_Behavior
//!    .adoc:155-156`) says *"prior to sending an NBIRTH message"*, and the same
//!    log shows the requested QoS is 1, which the clause also mandates.
//! 2. **The SubAck is read.** Asserted from the bridge's log, because the
//!    outcome of a subscription is only ever visible to the subscriber. AC2's
//!    whole point is that this must be legible *without* broker access, so the
//!    bridge's own log is the right oracle here — and the broker log
//!    independently confirms a SUBACK was sent.
//! 3. **An unrecognised command changes nothing.** Asserted two ways: the IGNORE
//!    trace (and, separately, that it names the metric), and — the load-bearing
//!    half — that NO second NBIRTH follows a `Node Control/Rebirth`. This bridge
//!    does not implement Rebirth (Story 4.7 does); a node that re-birthed here
//!    would have implemented it by accident.
//! 4. **The subscription is re-established on the SECOND connect too.** AC1's
//!    third clause binds every session, not the first. The reconnect is forced by
//!    connecting a second client under the bridge's own client id, so the BROKER
//!    evicts it — no container stop/start, which would remap the port.
//!
//! # Why a unit test cannot do this
//!
//! Packet order is a property of bytes on a socket. In-process, the SUBSCRIBE
//! and the PUBLISH are two enqueues onto the same channel, and asserting that
//! one call precedes another would only restate the source line above it. The
//! broker is the first component that sees them as packets.
//!
//! # Every way this test could pass for the wrong reason
//!
//! - *The broker is not verbose.* Then neither line exists, and both lookups
//!   fail loudly rather than defaulting to index 0. Checked by asserting each
//!   position is `Some` with its own message.
//! - *Another client's packets are counted.* The commander below publishes to a
//!   topic that CONTAINS the node id, so matching on the node id alone would
//!   match its line too. Both lookups match `from {NODE_ID}` as a phrase, and
//!   the publish lookup additionally requires `/NBIRTH/`, which no other client
//!   here ever publishes.
//! - *The bridge reconnects mid-test and re-births.* Then the "no second
//!   NBIRTH" check fires. That is a real failure of the assertion's premise, not
//!   a false one — but it would be reported as a Rebirth that never happened, so
//!   the message says so. (The DELIBERATE reconnect for property 4 happens after
//!   that check, for exactly this reason.)
//! - *The ignore traces appear without a command being received.* They cannot:
//!   every one of them is emitted only from the inbound-command arm.
//! - *The commands arrive before the subscription exists.* They cannot: the test
//!   waits for the NBIRTH first, and the NBIRTH is published after the SUBSCRIBE
//!   on the same connection, which the broker processes in order.
//! - *The metric-name assertion passes because something ACTED on the command.*
//!   It could, and it did — this was found by the Story 4.6 review. The needle
//!   was the metric name, which a Story 4.7 handler would print on the acting
//!   path too. The load-bearing assertion is now the phrase emitted only by the
//!   arm that throws the command away; the name is checked separately.
//! - *The bridge's DRIVER TASK dies but the process survives.* It can:
//!   `supervisor.rs` only awaits the mqtt task after shutdown, so
//!   `try_wait().is_none()` stays true and "no second NBIRTH" becomes trivially
//!   true — a dead driver publishes nothing. Also found by the review. A fourth
//!   command is sent AFTER those checks and its trace is required, which only a
//!   driver still in its loop can produce.
//!
//! # Falsification — run against deliberately broken code
//!
//! | Mutation | Result |
//! | --- | --- |
//! | `subscribe_to_commands` moved to AFTER the birth publish | RED — *"published its NBIRTH before subscribing"*, SUBSCRIBE at line 30, NBIRTH at 26 |
//! | the `Packet::SubAck` arm deleted, back to `Ok(_) => {}` | RED — *"never reported what the broker granted"* |
//! | the inbound-publish guard forced to `false` | RED — *"did not trace the command as IGNORED"* |
//! | `subscribe_to_commands` hoisted OUT of the `Transport::Connected` arm to a one-shot before the loop | RED — *"issued only 1 SUBSCRIBE in the whole run"* (added 2026-07-29 by the review; this mutation left the whole suite GREEN before property 4 existed) |
//! | the inbound-command `select!` arm made to `break` after the first command | RED — the liveness probe produces no trace (added 2026-07-29; this left the suite green before) |
//!
//! The first mutation is why the two-second settle above exists. Without it the
//! run went red for the wrong reason — it reported that the bridge issues no
//! SUBSCRIBE at all, because the late one had not yet reached the broker's log
//! when it was read. A test that fails is not automatically a test that tells
//! you what broke.

mod common;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};

const SERIAL: &str = "30000001";
const NODE_ID: &str = "ChaosNcmd";
const GROUP: &str = "ChaosNcmdGroup";

fn ncmd_topic() -> String {
    format!("spBv1.0/{GROUP}/NCMD/{NODE_ID}")
}

/// Kills the bridge if an assertion unwinds.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Removes the state directory on every path, including the failing ones.
struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A third client, used only to send commands at the bridge.
///
/// It waits for its own CONNACK before returning: a command published before
/// the broker has accepted the connection would be dropped by `rumqttc`'s queue
/// and the test would blame the bridge for never seeing it.
async fn commander(port: u16) -> AsyncClient {
    let mut options = MqttOptions::new("ncmd-commander", "127.0.0.1", port);
    options.set_keep_alive(Duration::from_secs(5));
    let (client, mut eventloop) = AsyncClient::new(options, 32);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
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

/// A Sparkplug payload naming `metrics`, encoded exactly as a Host Application
/// would send it.
fn command_payload(metrics: &[&str]) -> Vec<u8> {
    let metrics = metrics
        .iter()
        .map(|name| sparkplug_b::protobuf::payload::Metric {
            name: Some((*name).to_string()),
            ..Default::default()
        })
        .collect();
    sparkplug_b::encode(&sparkplug_b::protobuf::Payload {
        timestamp: Some(1_700_000_000_000),
        metrics,
        seq: None,
        uuid: None,
        body: None,
    })
}

/// Waits until the bridge's own log contains `needle`, or gives up.
///
/// Polls rather than reading once: the child writes the log, so a single read
/// races the line being flushed and would fail for a reason that has nothing to
/// do with the property under test.
async fn wait_for_log(path: &Path, needle: &str, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if std::fs::read_to_string(path)
            .map(|log| log.contains(needle))
            .unwrap_or(false)
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn log_tail(path: &Path) -> String {
    let log = std::fs::read_to_string(path).unwrap_or_else(|_| "<no log captured>".to_string());
    let tail: Vec<&str> = log.lines().rev().take(30).collect();
    tail.into_iter().rev().collect::<Vec<_>>().join("\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn chaos_ncmd_subscribed_before_the_birth_and_every_command_ignored() {
    let (broker, port) = common::start_verbose_broker().await;
    let mut seen = common::named_subscriber(port, "ncmd-observer").await;

    let state_dir = std::env::temp_dir().join(format!("chaos_ncmd_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);
    let log_path = state_dir.0.join("bridge.log");

    let log = File::create(&log_path).expect("capture the bridge's log");
    let log_err = log.try_clone().expect("clone the log handle");

    let child = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        // TEST-NET-1 (RFC 5737): unroutable, so the cloud stays silent. The
        // Sparkplug session does not depend on having a reading.
        .env("SMARTME_API_BASE", "https://192.0.2.1")
        .env("SMARTME_CLIENT_ID", "id")
        .env("SMARTME_CLIENT_SECRET", "secret")
        .env("SMARTME_METER_ID", "garage")
        .env("SMARTME_DEVICE_ID", "a1a1a1a1-b2b2-c3c3-d4d4-000000000001")
        .env("SMARTME_SERIAL", SERIAL)
        .env("SMARTME_GROUP_ID", GROUP)
        .env("SMARTME_NODE_ID", NODE_ID)
        .env("SMARTME_BROKER_HOST", "127.0.0.1")
        .env("SMARTME_BROKER_PORT", port.to_string())
        .env("SMARTME_STATE_DIR", state_dir.0.display().to_string())
        // The traces below ARE the acceptance criteria; a default filter that
        // dropped INFO would make this test assert the level, not the
        // behaviour.
        .env("RUST_LOG", "info")
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .expect("the bridge binary starts");
    let mut child = Reaped(child);

    let birth = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await;
    assert!(
        birth.is_some(),
        "no NBIRTH reached the observer, so nothing below can be judged.\n{}",
        log_tail(&log_path)
    );

    // ---------------------------------------------------------------- AC1 ---
    // Let the broker settle before reading its log.
    //
    // This delay is not cosmetic, and it was added because the falsification run
    // needed it. Reading the moment the NBIRTH arrives captures a log in which a
    // LATE subscribe has not landed yet — so a bridge that subscribes after
    // birthing reports as one that never subscribes at all, and the message
    // sends the next reader hunting the wrong bug. With the settle, "absent" and
    // "late" are distinguishable, which is the whole point of asserting the
    // order rather than the presence.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // The broker's record, in receipt order.
    let broker_log = String::from_utf8(
        broker
            .stderr_to_vec()
            .await
            .expect("the broker's log is readable"),
    )
    .expect("the broker logs utf-8");
    let lines: Vec<&str> = broker_log.lines().collect();

    let subscribe_at = lines
        .iter()
        .position(|line| line.contains(&format!("Received SUBSCRIBE from {NODE_ID}")));
    let birth_at = lines.iter().position(|line| {
        line.contains(&format!("Received PUBLISH from {NODE_ID} ")) && line.contains("/NBIRTH/")
    });

    let subscribe_at = subscribe_at.unwrap_or_else(|| {
        panic!(
            "the broker never received a SUBSCRIBE from {NODE_ID}. Either the bridge \
             issues none — which is what tck-id-message-flow-edge-node-ncmd-subscribe \
             forbids — or mosquitto is not running with -v and this test can prove \
             nothing.\nBroker log:\n{broker_log}"
        )
    });
    let birth_at = birth_at.unwrap_or_else(|| {
        panic!(
            "the broker received no NBIRTH from {NODE_ID}, so the ordering below has \
             no second term.\nBroker log:\n{broker_log}"
        )
    });

    assert!(
        subscribe_at < birth_at,
        "the bridge published its NBIRTH before subscribing to NCMD. \
         tck-id-message-flow-edge-node-ncmd-subscribe's preamble requires the \
         subscription PRIOR to the NBIRTH, and the plain reason is that a host \
         which answers a birth with a rebirth request would be talking to a node \
         that is not listening yet (SUBSCRIBE at line {subscribe_at}, NBIRTH at \
         line {birth_at}).\nBroker log:\n{broker_log}"
    );

    // The same clause mandates QoS 1 for this subscription. Taken from the
    // broker rather than from our own call site, which would only echo itself.
    //
    // Read from the line IMMEDIATELY AFTER the bridge's SUBSCRIBE, not from
    // anywhere in the log: mosquitto prints each requested filter under the
    // packet that carried it, so this ties the QoS to that SUBSCRIBE. A search
    // over the whole log would also accept the line if some other client had
    // happened to request the same filter.
    let expected = format!("{} (QoS 1)", ncmd_topic());
    let requested = lines.get(subscribe_at + 1).copied().unwrap_or("<nothing>");
    assert!(
        requested.ends_with(&expected),
        "the bridge's SUBSCRIBE did not request {} at QoS 1 — the broker recorded \
         {requested:?}. The clause is a MUST on QoS 1, and a QoS that is not the \
         one asked for is silent everywhere except here.\nBroker log:\n{broker_log}",
        ncmd_topic()
    );

    // ---------------------------------------------------------------- AC2 ---
    // The bridge read the answer instead of discarding it — the byte Story 4.4's
    // observer threw away, one file from here.
    assert!(
        wait_for_log(
            &log_path,
            "command subscription granted at QoS 1",
            Duration::from_secs(10)
        )
        .await,
        "the bridge never reported what the broker granted. A refused \
         subscription is return code 0x80 — not an error, not a disconnect — so \
         a bridge that does not read the SubAck cannot tell a refusal from a \
         quiet topic, and neither can the operator.\n{}",
        log_tail(&log_path)
    );

    // ---------------------------------------------------------------- AC3 ---
    let commander = commander(port).await;

    // The one command a live MQTT Engine actually sends. It must be IGNORED
    // here: Story 4.7 owns Rebirth.
    commander
        .publish(
            ncmd_topic(),
            QoS::AtLeastOnce,
            false,
            command_payload(&["Node Control/Rebirth"]),
        )
        .await
        .expect("the command is queued");

    // Bytes that are not a Sparkplug payload at all. Anyone can publish these.
    commander
        .publish(ncmd_topic(), QoS::AtLeastOnce, false, [0xffu8; 11].to_vec())
        .await
        .expect("the malformed command is queued");

    // Well-formed and empty: a shape that would otherwise read in a log as a
    // command whose names were lost.
    commander
        .publish(ncmd_topic(), QoS::AtLeastOnce, false, command_payload(&[]))
        .await
        .expect("the empty command is queued");

    // Assert the IGNORE TRACE, not the metric name.
    //
    // This needle was `"Node Control/Rebirth"` — the metric name — until the
    // Story 4.6 review pointed out that any future handler which ACTS on the
    // command would also print that name (`"answering Node Control/Rebirth"`),
    // keeping this assertion green while asserting the opposite of what its own
    // failure message says. The phrase below is emitted from one place only: the
    // arm that throws the command away.
    assert!(
        wait_for_log(
            &log_path,
            "unrecognised NCMD ignored",
            Duration::from_secs(15)
        )
        .await,
        "the bridge did not trace the command as IGNORED. Either it never arrived, \
         or — worse — something acted on it, which would mean Story 4.7 was \
         implemented here by accident.\n{}",
        log_tail(&log_path)
    );
    // And the trace names what arrived: AC3 requires the metric NAMES.
    assert!(
        wait_for_log(&log_path, "Node Control/Rebirth", Duration::from_secs(5)).await,
        "the ignore trace did not name the metric it saw, so the operator cannot \
         tell WHICH command was ignored.\n{}",
        log_tail(&log_path)
    );
    assert!(
        wait_for_log(
            &log_path,
            "not a Sparkplug payload was ignored",
            Duration::from_secs(15)
        )
        .await,
        "a payload that cannot be decoded was not traced; it was either applied \
         or silently dropped, and both are worse than no subscription at all.\n{}",
        log_tail(&log_path)
    );
    assert!(
        wait_for_log(
            &log_path,
            "carrying no metric was ignored",
            Duration::from_secs(15)
        )
        .await,
        "an NCMD with no metric was dropped silently.\n{}",
        log_tail(&log_path)
    );

    // The load-bearing half of "ignored": nothing happened. A Rebirth request
    // that produced an NBIRTH would mean Story 4.7 had been implemented here by
    // accident, with none of the evidence that story is going to require.
    let rebirth = common::wait_for(&mut seen, Duration::from_secs(5), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await;
    assert!(
        rebirth.is_none(),
        "a second NBIRTH followed the Node Control/Rebirth command. This story \
         builds plumbing that ignores every command; answering one here means the \
         behaviour exists without the conformance evidence Story 4.7 owns. (A \
         reconnect would also produce this, and would be worth knowing about too.)"
    );

    // And the bridge is still alive after all three: no panic, no exit.
    assert!(
        child
            .0
            .try_wait()
            .expect("the child's status is readable")
            .is_none(),
        "the bridge exited while handling commands it was supposed to ignore.\n{}",
        log_tail(&log_path)
    );

    // ------------------------------------------------- AC3, liveness, again ---
    // The check above is necessary and NOT sufficient, which the Story 4.6 review
    // found: `supervisor.rs` only awaits the mqtt task after shutdown is
    // signalled, so a driver task that panicked leaves the PROCESS running. Then
    // `try_wait().is_none()` passes, and `rebirth.is_none()` passes trivially —
    // a dead driver publishes nothing. Two assertions satisfied by the exact
    // failure they exist to exclude.
    //
    // So: send a fourth command AFTER all of the above and require its trace.
    // Only a driver still in its `select!` loop can produce it.
    commander
        .publish(
            ncmd_topic(),
            QoS::AtLeastOnce,
            false,
            command_payload(&["Chaos/Liveness Probe"]),
        )
        .await
        .expect("the liveness probe is queued");
    assert!(
        wait_for_log(&log_path, "Chaos/Liveness Probe", Duration::from_secs(15)).await,
        "the bridge process is alive but its MQTT driver task is not: a command \
         sent after the three above produced no trace. The process check alone \
         cannot see this, because the supervisor does not await the driver task \
         until shutdown.\n{}",
        log_tail(&log_path)
    );

    // ------------------------------------------- AC1, on the SECOND connect ---
    // AC1's third clause is *"re-established on EVERY reconnect, not only the
    // first"*, and nothing above tests it: the run so far has exactly one
    // connect. A refactor hoisting `subscribe_to_commands` out of the
    // `Transport::Connected` arm to a one-shot before the loop would leave every
    // assertion above green, and the bridge would then re-birth on each reconnect
    // WITHOUT re-subscribing — silently unreachable by any rebirth request for
    // the rest of its life.
    //
    // The reconnect is forced by connecting a second client under the bridge's
    // own client id (which is its node id, `supervisor.rs:104`). MQTT requires
    // the broker to evict the older session, so the bridge is disconnected by the
    // broker rather than by anything the test does to the container — and the
    // container keeps its port mapping, which a stop/start would not.
    let evictor = {
        let mut options = MqttOptions::new(NODE_ID, "127.0.0.1", port);
        options.set_keep_alive(Duration::from_secs(5));
        let (client, mut eventloop) = AsyncClient::new(options, 8);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
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
        ready_rx.await.expect("the evictor connected");
        client
    };
    // Let the broker complete the takeover, then step aside so the bridge's own
    // reconnect can succeed under its id.
    tokio::time::sleep(Duration::from_secs(1)).await;
    drop(evictor);

    // The bridge must come back, and the SECOND birth is the marker.
    let reborn = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await;
    assert!(
        reborn.is_some(),
        "the bridge never re-birthed after being evicted, so whether it \
         re-subscribes on a reconnect cannot be judged.\n{}",
        log_tail(&log_path)
    );

    // Same settle, same reason as the first read.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let broker_log = String::from_utf8(
        broker
            .stderr_to_vec()
            .await
            .expect("the broker's log is readable"),
    )
    .expect("the broker logs utf-8");
    let lines: Vec<&str> = broker_log.lines().collect();

    let subscribes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(&format!("Received SUBSCRIBE from {NODE_ID}")))
        .map(|(i, _)| i)
        .collect();
    let births: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            line.contains(&format!("Received PUBLISH from {NODE_ID} ")) && line.contains("/NBIRTH/")
        })
        .map(|(i, _)| i)
        .collect();

    assert!(
        subscribes.len() >= 2,
        "the bridge re-birthed after its reconnect but issued only \
         {} SUBSCRIBE(s) in the whole run. tck-id-message-flow-edge-node-ncmd-\
         subscribe binds every session, not the first one: a node that re-births \
         without re-subscribing is unreachable by any rebirth request for the rest \
         of its life, and nothing in its own log says so.\nBroker log:\n{broker_log}",
        subscribes.len()
    );
    assert!(
        births.len() >= 2,
        "the broker recorded fewer than two NBIRTHs, so the second connect was \
         not observed from the broker's side.\nBroker log:\n{broker_log}"
    );
    assert!(
        subscribes[1] < births[1],
        "on the SECOND connect the bridge published its NBIRTH before subscribing \
         (SUBSCRIBE at line {}, NBIRTH at line {}). The ordering clause binds every \
         session; holding it once is not holding it.\nBroker log:\n{broker_log}",
        subscribes[1],
        births[1]
    );

    // Stop it the way a container runtime does, so the run leaves nothing behind.
    let _ = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        if child.0.try_wait().expect("status readable").is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
