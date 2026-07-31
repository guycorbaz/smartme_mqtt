//! Stories 4.6 and 4.7 — the command subscription exists, is answered, one
//! command is honoured, and every other is thrown away safely.
//!
//! **This header said *"every command is thrown away safely"* and *"three
//! properties"* until the Story 4.7 code review.** Story 4.7 added a fifth
//! property that asserts a conformant command IS answered, and re-aimed three
//! assertions — while these four lines, four lines above the ones it amended, went
//! on describing the opposite. That is the sixth consecutive instance in this
//! project of a claim being corrected and the sentences that depend on it being
//! left alone, and the first one to happen inside a test file rather than a
//! document.
//!
//! Six properties, and each is asserted against a source OUTSIDE the bridge
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
//! 3. **A command this bridge does not implement changes nothing.** Asserted two
//!    ways: the IGNORE trace (and, separately, that it names the metric), and —
//!    the load-bearing half — that NO NBIRTH follows any of the four payloads
//!    sent, none of which is a conformant Rebirth Request.
//!
//!    **Story 4.7 re-aimed this rather than deleting it, and it is stronger
//!    now.** It used to fire on a `Node Control/Rebirth` and assert that nothing
//!    answered it, on the grounds that Rebirth was not implemented. Rebirth IS
//!    implemented, so the assertion is pointed at the near miss instead: a
//!    valueless rebirth metric, which `-ncmd-rebirth-value` says is not a
//!    request. A matcher that birthed on the metric NAME alone now goes red
//!    here — and under the old helper, which gave every metric `value: None`,
//!    that same mutation left this file green.
//! 4. **A conformant Rebirth Request IS answered** (Story 4.7), asserted at the
//!    very end of the run for the ordering reason recorded at its call site. The
//!    conformance evidence for the answer itself lives in
//!    `chaos_ncmd_rebirth.rs`.
//! 5. **The subscription is re-established on the SECOND connect too.** AC1's
//!    third clause binds every session, not the first. The reconnect is forced by
//!    connecting a second client under the bridge's own client id, so the BROKER
//!    evicts it — no container stop/start, which would remap the port.
//! 6. **A RETAINED Rebirth Request is a replay and is NOT answered** (ADR 0017,
//!    added by the Story 4.7 code review). `tck-id-payloads-ncmd-retain` forbids a
//!    host from retaining an NCMD, so one that arrives retained was replayed by the
//!    broker rather than sent by anyone — and answering it would make the bridge
//!    re-announce on every reconnect for as long as the retained message lives.
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
//!   the message says so. (The DELIBERATE reconnect for property 5 happens after
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
//!   arm that throws the command away; the name is checked separately, and the
//!   metric it names is one this bridge does NOT implement.
//! - *The "no NBIRTH" assertion holds because nothing it sent was ever a
//!   request.* **This was true of this file for the whole of Story 4.6, and it
//!   was the single most dangerous thing in it.** `command_payload` built
//!   metrics with `..Default::default()`, so `value: None`; under
//!   `tck-id-operational-behavior-data-commands-ncmd-rebirth-value` a valueless
//!   `Node Control/Rebirth` is not a Rebirth Request. So `rebirth.is_none()`
//!   stayed green after a perfect implementation AND after a completely broken
//!   one — it could not distinguish them, while its failure message warned about
//!   answering too eagerly and pointed the reader away from the real bug. The
//!   value is a parameter now, both shapes are sent, and they assert opposite
//!   things: the valueless one must NOT birth, the boolean-`true` one MUST.
//! - *The INFO assertions pass because the test set the log level itself.* They
//!   used to: this run set `RUST_LOG=info`, so it could never have noticed that
//!   the shipped default was ERROR and every one of these criteria was dark in a
//!   real deployment (the Story 4.6 review's finding D1). `RUST_LOG` is now
//!   removed from the child's environment, so these assertions ride on the
//!   default directive `main.rs` sets, and a regression there turns them red.
//! - *The conformant rebirth is sent early and shifts the broker-log indices.*
//!   It would: the second-connect checks index `births[1]` / `subscribes[1]` by
//!   ordinal, and an extra NBIRTH before them makes the ordering assertion
//!   compare two unrelated packets and fail for a reason its message does not
//!   describe. It is sent last, after those checks, and the comment at the call
//!   site says so.
//! - *The bridge's DRIVER TASK dies but the process survives.* It can:
//!   `supervisor.rs` only awaits the mqtt task after shutdown, so
//!   `try_wait().is_none()` stays true and "no second NBIRTH" becomes trivially
//!   true — a dead driver publishes nothing. Also found by the review. A fourth
//!   command is sent AFTER those checks and its trace is required, which only a
//!   driver still in its loop can produce.
//! - *The retained-NCMD check passes because the retain flag never arrives.*
//!   **The first version of property 6 failed for exactly this reason, and the
//!   production code was right.** Under MQTT 3.1.1 a broker sets the retain flag on
//!   DELIVERY only when the message is sent in answer to a new subscription; an
//!   ordinary live delivery to an already-subscribed client carries `retain = 0`
//!   whatever the publisher asked for. Publishing retained and asserting
//!   immediately therefore exercises the live path, where the flag is legitimately
//!   absent, and reads the resulting (correct) answer as a defect. The property is
//!   now provoked the way the real exposure is: publish retained, force a
//!   reconnect, and let the SUBSCRIBE draw the replay.
//! - *The retained check passes because a reconnect birth is counted as an
//!   answer.* It cannot: a reconnect legitimately produces an NBIRTH, so the
//!   assertion is on the COUNT of the answer's own trace not growing, never on the
//!   absence of an NBIRTH.
//!
//! # Falsification — run against deliberately broken code
//!
//! | Mutation | Result |
//! | --- | --- |
//! | `subscribe_to_commands` moved to AFTER the birth publish | RED — *"published its NBIRTH before subscribing"*, SUBSCRIBE at line 30, NBIRTH at 26 |
//! | the `Packet::SubAck` arm deleted, back to `Ok(_) => {}` | RED — *"never reported what the broker granted"* |
//! | the inbound-publish guard forced to `false` | RED — *"did not trace the command as IGNORED"* |
//! | `subscribe_to_commands` hoisted OUT of the `Transport::Connected` arm to a one-shot before the loop | RED — *"issued only 1 SUBSCRIBE in the whole run"* (added 2026-07-29 by the review; this mutation left the whole suite GREEN before property 5 existed) |
//! | the inbound-command `select!` arm made to `break` after the first command | RED — the liveness probe produces no trace (added 2026-07-29; this left the suite green before) |
//! | `classify` widened to match `Node Control/Rebirth` on the NAME ALONE | RED — *"a Node Control/Rebirth carrying no value was not reported as a near miss"*. **Not the assertion this table first predicted:** the near-miss trace check sits earlier in the run than the "no NBIRTH" one and fires first. Both would have caught it; the recorded result is the one that actually ran. (added 2026-07-30; under the old `..Default::default()` helper this mutation left the file GREEN) |
//! | the `Inbound::Rebirth` arm of the command branch deleted | RED — *"a conformant Node Control/Rebirth (boolean true) produced no NBIRTH"* (added 2026-07-30) |
//! | `main.rs`'s default directive reverted to `tracing_subscriber::fmt::init()` (default ERROR) | RED — at the FIRST INFO assertion, *"the bridge never reported what the broker granted"*, because the run no longer sets `RUST_LOG`. Before Story 4.7 this mutation left the file green. (added 2026-07-30) |
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

/// A Sparkplug payload naming `metrics` with their values, encoded exactly as a
/// Host Application would send it.
///
/// # Why the value became a parameter (Story 4.7)
///
/// It used to build every metric with `..Default::default()`, so `value: None`.
/// That was invisible while nothing acted on any command, and it became the most
/// dangerous thing in this file the moment something did.
///
/// `tck-id-operational-behavior-data-commands-ncmd-rebirth-value` defines a
/// Rebirth Request as carrying the boolean value `true`, and this bridge
/// implements that reading. So a valueless `Node Control/Rebirth` is NOT a
/// request and must not be answered — which means the old `rebirth.is_none()`
/// assertion below stayed green after a perfect implementation *and* after a
/// completely broken one. It could not tell them apart, and its failure message
/// warned about answering too eagerly, pointing the next reader in the opposite
/// direction from the bug. A dev reading the green suite as validation would
/// have shipped a bridge that answers nothing.
///
/// Both shapes are now sent, on purpose, and they assert opposite things.
///
/// # The datatype is DERIVED from the value, not from its presence
///
/// The first version of this helper wrote
/// `datatype: value.as_ref().map(|_| DataType::Boolean.code())` — hard-coding
/// Boolean for whatever it was handed. Under a doc claiming the payload is
/// *"encoded exactly as a Host Application would send it"*, any caller passing a
/// non-boolean would silently get a metric whose DECLARED datatype contradicts its
/// value: the exact near-miss shape this story exists to detect, manufactured by
/// the helper meant to reproduce a conformant host. That is the same class of trap
/// as the `..Default::default()` one documented above, and the Story 4.7 code
/// review found it in the file that had just spent forty lines on the first.
fn command_payload(
    metrics: &[(&str, Option<sparkplug_b::protobuf::payload::metric::Value>)],
) -> Vec<u8> {
    use sparkplug_b::protobuf::payload::metric::Value;
    let metrics = metrics
        .iter()
        .map(|(name, value)| sparkplug_b::protobuf::payload::Metric {
            name: Some((*name).to_string()),
            datatype: value.as_ref().map(|v| {
                match v {
                    Value::BooleanValue(_) => sparkplug_b::DataType::Boolean,
                    Value::IntValue(_) => sparkplug_b::DataType::Int32,
                    Value::LongValue(_) => sparkplug_b::DataType::Int64,
                    Value::FloatValue(_) => sparkplug_b::DataType::Float,
                    Value::DoubleValue(_) => sparkplug_b::DataType::Double,
                    Value::StringValue(_) => sparkplug_b::DataType::String,
                    // Deliberately loud rather than defaulted: a caller reaching
                    // for a variant this helper cannot honestly type must decide
                    // what the host would declare, not inherit Boolean by
                    // accident.
                    other => panic!(
                        "command_payload cannot infer a datatype for {other:?}; add \
                         the mapping rather than letting the metric declare a type \
                         it does not carry"
                    ),
                }
                .code()
            }),
            is_null: value.is_none().then_some(true),
            value: value.clone(),
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

/// The one command this bridge implements, in its conformant form.
fn rebirth_request() -> Vec<u8> {
    command_payload(&[(
        "Node Control/Rebirth",
        Some(sparkplug_b::protobuf::payload::metric::Value::BooleanValue(
            true,
        )),
    )])
}

/// Forces the broker to evict the bridge, so it reconnects and RE-SUBSCRIBES.
///
/// Connecting a second client under the bridge's own client id (its node id,
/// `supervisor.rs:104`) makes the broker take the session away, per MQTT. The
/// container keeps its port mapping, which a stop/start would not.
///
/// Both halves of the teardown matter, and the abort is the one that does.
/// Dropping the client alone does NOT stop the eviction: the spawned task still
/// owns the `EventLoop` and `rumqttc` reconnects internally, so it goes on
/// re-taking the bridge's id roughly once a second for the rest of the run. That
/// was invisible while this test ended immediately afterwards, and stopped being
/// invisible when Story 4.7 put a command at the end of it.
async fn evict_and_wait_for_resubscribe(port: u16) {
    let (evictor, evictor_loop) = {
        let mut options = MqttOptions::new(NODE_ID, "127.0.0.1", port);
        options.set_keep_alive(Duration::from_secs(5));
        let (client, mut eventloop) = AsyncClient::new(options, 8);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
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
        (client, handle)
    };
    // Let the broker complete the takeover, then step aside so the bridge's own
    // reconnect can succeed under its id.
    tokio::time::sleep(Duration::from_secs(1)).await;
    drop(evictor);
    evictor_loop.abort();
    // And let the bridge settle back onto its id before anything is sent to it.
    tokio::time::sleep(Duration::from_secs(2)).await;
}

/// Polls `condition` until it holds, or gives up.
///
/// For properties about a COUNT rather than a phrase, where `wait_for_log`'s
/// substring search cannot express what is being waited for.
async fn wait_for(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    condition()
}

/// How many times `needle` appears in the bridge's log.
///
/// Presence is not enough where a run legitimately produces the line more than
/// once: the retained-NCMD assertion needs to know the count did NOT grow.
fn count_in_log(path: &Path, needle: &str) -> usize {
    std::fs::read_to_string(path)
        .map(|log| log.matches(needle).count())
        .unwrap_or(0)
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
        // NO `RUST_LOG`, deliberately, and it is removed from the inherited
        // environment so an ambient one cannot leak in (Story 4.7).
        //
        // It used to be set to `info` here, with a comment explaining that the
        // traces below ARE the acceptance criteria. That was right about the
        // criteria and wrong about how to test them: a run that sets the filter
        // itself asserts what an operator would see *if they configured it*,
        // and the Story 4.6 review's finding D1 was that they would not — the
        // default was ERROR, so every criterion written in terms of an INFO
        // trace was dark in a real deployment while this file was green.
        //
        // `main.rs` now sets an explicit INFO default directive. Letting this
        // test depend on it is what CONFIRMS that, every run, instead of
        // assuming it: if the default regresses, the INFO assertions below go
        // red. That is the only way this file can notice.
        .env_remove("RUST_LOG")
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

    // A command this bridge genuinely does not implement.
    //
    // This used to be `Node Control/Rebirth`, which is no longer unrecognised —
    // Story 4.7 implements it. The needle below is emitted only by the arm that
    // throws a command away, so it now needs a command that is actually thrown
    // away, or it would be asserting nothing.
    commander
        .publish(
            ncmd_topic(),
            QoS::AtLeastOnce,
            false,
            command_payload(&[("Node Control/Next Server", None)]),
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

    // The NEAR MISS (Story 4.7 / AC6): the rebirth metric by name, with no
    // value — the exact payload `..Default::default()` used to build silently.
    // `-ncmd-rebirth-value` requires the boolean value `true`, so this is not a
    // Rebirth Request and must NOT produce a birth.
    commander
        .publish(
            ncmd_topic(),
            QoS::AtLeastOnce,
            false,
            command_payload(&[("Node Control/Rebirth", None)]),
        )
        .await
        .expect("the valueless rebirth is queued");

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
    //
    // The needle was `Node Control/Rebirth` until Story 4.7, and it is no longer
    // discriminating: the ANSWER trace names that metric too, so this assertion
    // would now pass on a bridge that acted on every command. It is aimed at the
    // command that is genuinely ignored instead.
    assert!(
        wait_for_log(
            &log_path,
            "Node Control/Next Server",
            Duration::from_secs(5)
        )
        .await,
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

    // Story 4.7 / AC6 — the NEAR MISS is traced with what actually arrived.
    //
    // This is the whole mitigation for implementing the norm's reading
    // literally. A strict matcher that is wrong about the encoding never fires,
    // SILENTLY, and the bridge then reports FR19 as implemented with nothing
    // observably wrong. The datatype and value in this line are what make such a
    // failure diagnosable in one grep instead of invisible.
    assert!(
        wait_for_log(
            &log_path,
            "is not a Rebirth Request",
            Duration::from_secs(15)
        )
        .await,
        "a Node Control/Rebirth carrying no value was not reported as a near miss. \
         Either it was answered — which -ncmd-rebirth-value forbids — or it was \
         swallowed into the ordinary ignore path, where a host whose encoding this \
         bridge does not accept looks exactly like a host that never asked.\n{}",
        log_tail(&log_path)
    );

    // The load-bearing half of "ignored", RE-AIMED rather than deleted.
    //
    // It used to guard "this story implements no command", and a Rebirth request
    // was what it fired on. Story 4.7 implements that command, so the assertion
    // is now pointed at the four payloads above — none of which is a conformant
    // Rebirth Request — and it has become STRONGER than it was.
    //
    // The reason is the trap this file used to contain: `command_payload` built
    // metrics with no value, so the rebirth it published was never a request in
    // the first place. The old assertion therefore held whatever the bridge did,
    // and confirmed nothing. It now says something a broken bridge can violate:
    // a matcher that answers on the NAME ALONE would birth here and go red, and
    // that is exactly the liberal reading this project decided against.
    let birth_on_a_non_request = common::wait_for(&mut seen, Duration::from_secs(5), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await;
    assert!(
        birth_on_a_non_request.is_none(),
        "an NBIRTH followed a command that is not a Rebirth Request. \
         tck-id-operational-behavior-data-commands-ncmd-rebirth-value defines a \
         request as carrying the boolean value true; a bridge that births on the \
         metric NAME alone would re-announce every time a host echoed our own \
         declaration back at us. (A reconnect would also produce this, and would be \
         worth knowing about too.)"
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
            command_payload(&[("Chaos/Liveness Probe", None)]),
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
    evict_and_wait_for_resubscribe(port).await;

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

    // ------------------------------------------------- Story 4.7, inverted ---
    // The assertion this file used to make in reverse.
    //
    // A CONFORMANT Rebirth Request — the name AND the boolean value `true` — must
    // now produce a birth. `tck-id-operational-behavior-data-commands-rebirth-\
    // action-2` requires a complete BIRTH sequence on receipt.
    //
    // It is sent HERE, last, and not next to the other commands, for a reason
    // that is easy to get wrong: the second-connect assertions above index into
    // the broker's log by ordinal (`births[1]`, `subscribes[1]`) to prove the
    // SUBSCRIBE preceded the NBIRTH on the reconnect. An extra NBIRTH published
    // before that point would shift those indices and the ordering check would
    // compare the rebirth's birth against the reconnect's subscribe — failing,
    // and failing with a message about packet ordering that has nothing to do
    // with the actual cause.
    //
    // The end-to-end conformance evidence for the answer (complete sequence,
    // seq restart, unchanged bdSeq, no DATA interleaved) lives in
    // `chaos_ncmd_rebirth.rs`, which owns AC2/AC3/AC5. What is asserted here is
    // narrower and still worth having: the same binary, on a connection that has
    // already reconnected once, still answers.
    // DRAIN FIRST, and this is not tidiness.
    //
    // The run has just forced a broker eviction to prove property 5, so the
    // receiver holds the RECONNECT birth — an NBIRTH that has nothing to do with
    // any command. `wait_for` accepts the first match it finds, so without this
    // drain the assertion below would be satisfied by that leftover, and by any
    // further reconnect during its 20-second window. Found by the Story 4.7 code
    // review: an assertion that accepts *any* NBIRTH cannot distinguish an answer
    // from a reconnect, which is the one distinction it exists to make.
    common::drain(&mut seen).await;

    commander
        .publish(ncmd_topic(), QoS::AtLeastOnce, false, rebirth_request())
        .await
        .expect("the conformant rebirth request is queued");

    // The ANSWER's own trace, not the classification's.
    //
    // `"Rebirth Request accepted"` is emitted by `trace_command_outcome`, which
    // runs BEFORE `announce` is called — so it proves only that the bytes were
    // understood. It stays green if the `Inbound::Rebirth` arm is deleted, and if
    // `announce` refuses to birth and publishes nothing. The Story 4.7 code review
    // found this file resting on that needle plus an any-NBIRTH check, neither of
    // which witnesses the answer. `announce`'s own line is emitted only after the
    // whole sequence has been queued.
    assert!(
        wait_for_log(
            &log_path,
            "node re-announced on a Rebirth Request",
            Duration::from_secs(10)
        )
        .await,
        "the bridge never traced an ANSWER. Note what this does NOT accept: the \
         classification trace (\"Rebirth Request accepted\") fires before the birth \
         is attempted, so it proves the bytes were read and nothing more. An \
         operator reading this log must be able to tell a host-requested \
         re-announcement from a reconnect, and those have entirely different causes \
         to go looking for. (No RUST_LOG is set in this run, so this also confirms \
         the INFO default in main.rs is still in force.)\n{}",
        log_tail(&log_path)
    );
    let answered = common::wait_for(&mut seen, Duration::from_secs(20), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await;
    assert!(
        answered.is_some(),
        "a conformant Node Control/Rebirth (boolean true) produced no NBIRTH on the \
         wire. The subscription is live on a broker where MQTT Engine sends real \
         Rebirth requests, so a request that is received, classified and then \
         dropped is the failure FR19 exists to fix — and it is invisible from the \
         host's side, which cannot distinguish it from a node that never heard.\n{}",
        log_tail(&log_path)
    );

    // ------------------------------------------------------- property 6 ---
    // A RETAINED conformant request is a REPLAY and must not be answered
    // (ADR 0017).
    //
    // # The delivery has to be provoked, and getting this wrong is instructive
    //
    // The first version of this block published the request with `retain = true`
    // and asserted immediately. It failed — and the production code was right.
    // Under MQTT 3.1.1 the broker sets the retain flag on DELIVERY only when the
    // message is sent in response to a new subscription; an ordinary live delivery
    // to an already-subscribed client carries `retain = 0`, whatever the publisher
    // asked for. So that test exercised the live path, where the flag is
    // legitimately absent, and read the resulting answer as a defect.
    //
    // The dangerous path is the replay, and it is reached by making the bridge
    // SUBSCRIBE again. That is also the real attack: publish once, walk away, and
    // every future session of the bridge is handed the request at connect time,
    // for as long as the retained message exists.
    //
    // `tck-id-payloads-ncmd-retain` (`Sparkplug_6_Payloads.adoc:1421`) — *"NCMD
    // messages MUST be published with the MQTT retain flag set to false"* — is what
    // makes rejecting it free: no conformant Host Application sends one.
    // # The same bytes are answered once and refused once, and that is the point
    //
    // Publishing with `retain = true` does TWO things: it delivers the message to
    // current subscribers, and it stores it for future ones. The live delivery
    // reaches the bridge with `retain = 0` — see above — so it is a request like
    // any other and IS answered. Correctly: someone published it just now.
    //
    // The stored copy is the exposure, and it is drawn out by the reconnect below.
    // So the count is snapshotted AFTER the live answer has landed, and what must
    // not grow is the count across the REPLAY. Getting this wrong is how the first
    // version of this block failed: it snapshotted before the live delivery and
    // then blamed the bridge for answering a request that had genuinely been sent.
    let answers_at_start = count_in_log(&log_path, "node re-announced on a Rebirth Request");
    commander
        .publish(ncmd_topic(), QoS::AtLeastOnce, true, rebirth_request())
        .await
        .expect("the retained rebirth request is queued");
    assert!(
        wait_for(Duration::from_secs(20), || {
            count_in_log(&log_path, "node re-announced on a Rebirth Request") > answers_at_start
        })
        .await,
        "the LIVE delivery of the retained publish was not answered. It arrives with \
         retain=0 (the broker sets the flag only on a subscription replay), so it is \
         an ordinary request and refusing it would be over-broad.\n{}",
        log_tail(&log_path)
    );
    let answers_before_replay = count_in_log(&log_path, "node re-announced on a Rebirth Request");

    evict_and_wait_for_resubscribe(port).await;

    assert!(
        wait_for_log(&log_path, "RETAIN flag set", Duration::from_secs(30)).await,
        "the bridge re-subscribed, the broker replayed the retained \
         Node Control/Rebirth, and nothing said so. If it is silently answered \
         instead, one publish by any client on this unauthenticated broker makes \
         every future session answer a request nobody is sending — with no attacker \
         present and nothing in the log to distinguish it from a real host.\n{}",
        log_tail(&log_path)
    );
    // The count, not the presence: a reconnect legitimately produces an NBIRTH, so
    // "no NBIRTH" cannot be asserted across one. What must not have happened is
    // another ANSWER.
    assert_eq!(
        count_in_log(&log_path, "node re-announced on a Rebirth Request"),
        answers_before_replay,
        "a REPLAYED Node Control/Rebirth was answered. The broker hands a retained \
         message to every subscriber on every SUBSCRIBE, so answering one is \
         self-sustaining: it fires again on the next reconnect, and the one after \
         that, indefinitely.\n{}",
        log_tail(&log_path)
    );
    // Clear it. The container dies with the test, so this is hygiene rather than
    // necessity — but a retained command left on a broker is exactly the state
    // this property exists to warn about, and leaving one behind in a test that
    // teaches its danger would be poor form. An empty payload is how MQTT deletes
    // a retained message.
    commander
        .publish(ncmd_topic(), QoS::AtLeastOnce, true, Vec::new())
        .await
        .expect("the retained message is cleared");

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
