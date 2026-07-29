//! Story 4.4 — observe what a real Sparkplug Host Application publishes on
//! `spBv1.0/STATE/…`, so that Story 4.5 can decide with evidence rather than
//! with a reading of the specification.
//!
//! # This publishes NOTHING
//!
//! The only broker available is production, and Ignition is live on it. This
//! test subscribes and prints. It registers no will, sends no birth, and never
//! calls `publish`. Read the transcript, not the code, to learn what the host
//! does — that is the entire point of the story.
//!
//! # Why this does not reuse `common::named_subscriber_on`
//!
//! That helper exists, talks to a real broker, and is the wrong tool. It decodes
//! every payload as Sparkplug protobuf and silently drops what fails:
//!
//! ```text
//! if let Ok(payload) = sparkplug_b::decode(&p.payload) { ... }   // no else
//! ```
//!
//! **STATE payloads are JSON, not protobuf.** Reusing it would show an empty
//! transcript on a busy topic and support the conclusion "the host publishes no
//! STATE" — a false negative of exactly the class that produced the contract-v1
//! quality codes. Its `Seen` struct also discards `retain` and `qos`, and this
//! story needs both: retained delivery is how a stored snapshot is told apart
//! from a live transition.
//!
//! # Every way this instrument can lie, and what stops it
//!
//! Added after the Story 4.4 review, which found that the first version could
//! report silence for four different reasons and name none of them. An observer
//! whose failure mode is an empty transcript is worse than no observer, because
//! an empty transcript reads as an answer.
//!
//! - **The broker denies the subscription.** MQTT 3.1.1 answers a refused
//!   SUBSCRIBE with a SubAck return code `0x80`, not with an error. The first
//!   version matched `Packet::SubAck(_)` and threw the codes away, so a denied
//!   subscription looked exactly like a quiet topic — and the run then printed a
//!   checklist telling the operator to "rule out a broker ACL", which is the
//!   question the discarded byte had already answered. Now: a denial fails the
//!   run immediately, quoting the broker.
//! - **The broker grants a lower QoS than requested.** The SubAck carries the
//!   *granted* QoS. A downgrade to 0 makes every delivered `qos` read
//!   `AtMostOnce`, which would be recorded as a host non-conformance that never
//!   happened. Now: reported, and the transcript says the reading is void.
//! - **The observer task dies mid-window.** A closed channel and an elapsed
//!   window took the same code path, and the summary printed the full nominal
//!   window either way. A run that died 30 s into 900 s produced a confident
//!   "nothing was published". Now: tracked separately and printed first.
//! - **The transport drops and reconnects.** With `clean_session(true)` anything
//!   published during the gap is lost, not queued, so an invisible gap is a
//!   silent hole in the evidence. Now: counted and reported.
//! - **A duplicate delivery.** At QoS 1 a redelivery is indistinguishable from a
//!   second publish unless `dup` is captured. Story 4.4 recorded "published
//!   twice" without it. Now: `dup` and `pkid` are captured and printed.
//! - **An empty payload.** A zero-length retained publish is how MQTT *clears* a
//!   retained message. The first version reported it as a payload that
//!   "contradicts the specification".
//!
//! # Running it
//!
//! ```text
//! SMARTME_STATE_BROKER=host:1883 \
//!   cargo test -p smartme-bridge --test observe_primary_host_state \
//!   -- --ignored --nocapture
//! ```
//!
//! Optional:
//! - `SMARTME_STATE_FILTER` — topic filter, default `spBv1.0/STATE/#`. Re-run
//!   with `#` to rule out an ACL or a non-standard topic shape before concluding
//!   that a quiet transcript means a quiet host.
//! - `SMARTME_STATE_SECONDS` — observation window, default 90. A value that does
//!   not parse is a hard error, not a fallback: see the note at its parse site.
//! - `SMARTME_STATE_CLIENT_ID` — set it whenever two observers run at once.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, SubscribeReasonCode};
use tokio::sync::mpsc;

/// One message exactly as the broker delivered it.
///
/// Raw bytes, not a decoded payload: the whole failure this test exists to avoid
/// is a decoder deciding which messages are worth showing you.
#[derive(Debug, Clone)]
struct Observed {
    /// Milliseconds since epoch, from the observer's own clock. This is a
    /// *receive* time and is deliberately kept separate from any timestamp
    /// found inside the payload, which the specification defines as a
    /// CONNECT-time value rather than a publish time.
    received_at_ms: u128,
    topic: String,
    payload: Vec<u8>,
    /// Retained by the broker. `true` means "stored snapshot replayed to a new
    /// subscriber", `false` means "published while we were watching".
    ///
    /// Note the asymmetry, which the write-up must respect: `retain=true` at
    /// subscribe proves a stored snapshot, but `retain=false` does NOT prove the
    /// message was published unretained. MQTT 3.1.1 requires the broker to clear
    /// the flag when it delivers a retained message to an *already-subscribed*
    /// client, so a live delivery is silent about how it was published.
    retain: bool,
    qos: QoS,
    /// The MQTT DUP flag. Set when the broker is *redelivering* a QoS>0 message
    /// it believes was not acknowledged. Without it, "the host published twice"
    /// and "the broker delivered once, twice" cannot be told apart.
    dup: bool,
    /// Packet id. Two deliveries sharing a pkid are one message.
    pkid: u16,
}

impl Observed {
    /// The payload as text when it is valid UTF-8, hex otherwise. No decoding
    /// is attempted or implied.
    fn body(&self) -> String {
        match std::str::from_utf8(&self.payload) {
            Ok(text) => text.to_string(),
            Err(_) => self
                .payload
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(""),
        }
    }

    /// The last topic token — the `sparkplug_host_id` for a STATE topic.
    fn last_token(&self) -> &str {
        self.topic.rsplit('/').next().unwrap_or("")
    }

    /// The payload's keys and value types, when it parses as a JSON object.
    /// Reports what is *actually* there, not what the specification predicts.
    fn json_shape(&self) -> Option<String> {
        let value: serde_json::Value = serde_json::from_slice(&self.payload).ok()?;
        let object = value.as_object()?;
        let mut parts: Vec<String> = object
            .iter()
            .map(|(k, v)| {
                let kind = match v {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Bool(_) => "bool",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::String(_) => "string",
                    serde_json::Value::Array(_) => "array",
                    serde_json::Value::Object(_) => "object",
                };
                format!("{k}: {kind}")
            })
            .collect();
        parts.sort();
        Some(parts.join(", "))
    }
}

/// Everything the observer task tells the run, not only the messages.
///
/// The first version carried messages alone, which is why a dead task and a
/// quiet broker were indistinguishable downstream.
#[derive(Debug)]
enum Signal {
    Message(Box<Observed>),
    /// A ConnAck. The first is the connection; every later one is a reconnect,
    /// and with `clean_session(true)` the gap before it swallowed anything the
    /// host published.
    Connected,
    /// The broker's answer to our SUBSCRIBE, verbatim.
    Granted(Vec<SubscribeReasonCode>),
    /// A transport error, with the broker's own words.
    Transport(String),
    /// The task is stopping and will send nothing further. Carries why.
    Ended(String),
}

/// What the run actually did, as opposed to what it was asked to do.
#[derive(Debug, Default)]
struct Run {
    seen: Vec<Observed>,
    /// ConnAcks after the first.
    reconnects: usize,
    transport_errors: Vec<String>,
    /// `Some(reason)` when the observer stopped before the window elapsed.
    ended_early: Option<String>,
    /// The QoS the broker actually granted, when it differs from what we asked.
    downgraded_to: Option<QoS>,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the epoch")
        .as_millis()
}

/// Subscribes and streams raw messages. Never publishes.
async fn observe(host: &str, port: u16, filter: &str, client_id: &str) -> mpsc::Receiver<Signal> {
    let mut options = MqttOptions::new(client_id, host, port);
    options.set_keep_alive(Duration::from_secs(10));
    // Set explicitly rather than inherited. rumqttc happens to default this to
    // true, and #35 records that relying on that default is how a MUST goes
    // unasserted — a lesson this story is not going to re-learn on a production
    // broker, where a persistent session would queue messages for a client id
    // that never comes back.
    options.set_clean_session(true);

    let (client, mut eventloop) = AsyncClient::new(options, 256);
    let (tx, rx) = mpsc::channel(1024);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    // The task owns a copy; `filter` itself stays borrowable for the diagnostics
    // below, which must be able to name the filter the broker refused.
    let task_filter = filter.to_string();

    tokio::spawn(async move {
        let mut ready_tx = Some(ready_tx);
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    if tx.send(Signal::Connected).await.is_err() {
                        return;
                    }
                    // QoS 1: the host publishes STATE at QoS 1, and subscribing
                    // lower would silently downgrade the delivery we are here to
                    // characterise.
                    //
                    // NOT `.expect()`. This runs on every ConnAck, so a failure
                    // on a mid-window reconnect used to panic the task — after
                    // `ready` had already been sent, which meant the run carried
                    // on and reported a full window it had not observed.
                    if let Err(error) = client
                        .subscribe(task_filter.clone(), QoS::AtLeastOnce)
                        .await
                    {
                        let _ = tx
                            .send(Signal::Ended(format!(
                                "the SUBSCRIBE could not be sent: {error}"
                            )))
                            .await;
                        return;
                    }
                }
                Ok(Event::Incoming(Packet::SubAck(ack))) => {
                    if tx
                        .send(Signal::Granted(ack.return_codes.clone()))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    if let Some(tx) = ready_tx.take() {
                        let _ = tx.send(ack.return_codes.clone());
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    // EVERY message, decoded by nothing.
                    let observed = Observed {
                        received_at_ms: now_ms(),
                        topic: p.topic.clone(),
                        payload: p.payload.to_vec(),
                        retain: p.retain,
                        qos: p.qos,
                        dup: p.dup,
                        pkid: p.pkid,
                    };
                    if tx.send(Signal::Message(Box::new(observed))).await.is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("  [transport] {error}");
                    if tx.send(Signal::Transport(error.to_string())).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });

    // A bounded wait, not an open one. An unreachable broker otherwise parks
    // here forever with no diagnostic, and the operator learns nothing except
    // that nothing happened — during a restart window that cannot be repeated
    // cheaply. rumqttc retries the connection internally, so "no SubAck within
    // the deadline" means the broker is unreachable, refusing us, or slow enough
    // to matter.
    const CONNECT_DEADLINE: Duration = Duration::from_secs(15);
    let granted = match tokio::time::timeout(CONNECT_DEADLINE, ready_rx).await {
        Ok(Ok(codes)) => codes,
        Ok(Err(_)) => {
            panic!("the observer task ended before it subscribed — see [transport] above")
        }
        Err(_) => panic!(
            "no SubAck within {}s. The broker did not accept a subscription.\n  \
             Check, in this order:\n    \
             1. is SMARTME_STATE_BROKER reachable from here at all (`nc -vz host port`)?\n    \
             2. is a sandbox or firewall blocking the connection?\n    \
             3. does the broker require credentials this observer does not send?\n    \
             4. does an ACL forbid this client id from subscribing?\n  \
             The [transport] lines above carry the broker's own words.",
            CONNECT_DEADLINE.as_secs()
        ),
    };

    // The broker answered. If it refused, say so NOW rather than waiting out the
    // window and then reporting silence: a denied subscription cannot deliver a
    // message, so there is no transcript to lose by failing here. This is the
    // single correction that matters most in this file — the first version threw
    // these codes away and then asked the operator to rule out, by hand, the
    // very thing the broker had just told it.
    if granted.contains(&SubscribeReasonCode::Failure) {
        panic!(
            "the broker REFUSED the subscription to {filter:?} (SubAck 0x80).\n  \
             This is not a quiet topic — nothing can ever arrive on this filter for\n  \
             this client id. Do NOT record 'the host publishes no STATE' from this run.\n  \
             Most likely an ACL on the broker, or a client id it does not accept."
        );
    }

    rx
}

/// Prints the transcript and the diagnostics a quiet run needs in order to mean
/// anything.
///
/// `observed_for` is what the run actually lasted, which is not the requested
/// window whenever the observer stopped early.
fn report(run: &Run, filter: &str, window: Duration, observed_for: Duration) {
    // Anything that makes the transcript mean less than it appears to goes
    // first, before the reader has formed an impression from the messages.
    if let Some(reason) = &run.ended_early {
        println!(
            "\n!!! THE OBSERVER STOPPED EARLY — after {}s of a {}s window.\n    \
             Reason: {reason}\n    \
             The transcript below is a PARTIAL record. It cannot support any claim\n    \
             about what was not published, and 'live: 0' here means nothing.",
            observed_for.as_secs(),
            window.as_secs()
        );
    }
    if let Some(qos) = run.downgraded_to {
        println!(
            "\n!!! THE BROKER GRANTED {qos:?}, NOT AtLeastOnce.\n    \
             Every `qos=` below is the granted QoS, not the published one, so this\n    \
             run CANNOT be used to check a QoS clause against the host."
        );
    }
    if run.reconnects > 0 {
        println!(
            "\n!!! {} RECONNECT(S) DURING THE WINDOW.\n    \
             clean_session is true, so nothing published during those gaps was\n    \
             queued for us. There is a hole in this evidence of unknown width.",
            run.reconnects
        );
    }

    let seen = &run.seen;
    println!("\n=== TRANSCRIPT ({} message(s)) ===", seen.len());
    for (i, m) in seen.iter().enumerate() {
        println!(
            "\n[{i}] +{}ms  retain={}  qos={:?}  dup={}  pkid={}\n     topic   : {}\n     host_id : {}\n     payload : {:?} ({} bytes)",
            // saturating: `received_at_ms` is wall-clock, so an NTP step
            // backwards mid-window would underflow this and panic in a debug
            // build — destroying the whole transcript AFTER the observation and
            // BEFORE it was printed.
            m.received_at_ms.saturating_sub(seen[0].received_at_ms),
            m.retain,
            m.qos,
            m.dup,
            m.pkid,
            m.topic,
            m.last_token(),
            m.body(),
            m.payload.len()
        );
        if m.payload.is_empty() {
            println!(
                "     json    : EMPTY payload — in MQTT this is how a retained message is\n               \
                 CLEARED. Record it as a host clearing its STATE, not as a malformed one."
            );
        } else {
            match m.json_shape() {
                Some(shape) => println!("     json    : {{ {shape} }}"),
                None => println!(
                    "     json    : not a JSON object. The Sparkplug 3.0 STATE form requires\n               \
                     JSON; a legacy or non-Sparkplug form need not. Record which this is."
                ),
            }
        }
    }

    let retained = seen.iter().filter(|m| m.retain).count();
    let live = seen.len() - retained;
    let redeliveries = seen.iter().filter(|m| m.dup).count();
    println!("\n=== SUMMARY ===");
    println!("  filter            : {filter}");
    println!("  window requested  : {}s", window.as_secs());
    println!("  actually observed : {}s", observed_for.as_secs());
    println!("  retained (snapshot): {retained}");
    println!("  live (published)   : {live}");
    println!("  flagged dup        : {redeliveries}");
    println!("  reconnects         : {}", run.reconnects);
    println!("  transport errors   : {}", run.transport_errors.len());
    let mut hosts: Vec<&str> = seen.iter().map(|m| m.last_token()).collect();
    hosts.sort_unstable();
    hosts.dedup();
    println!("  distinct host ids  : {hosts:?}");
    let mut topics: Vec<&str> = seen.iter().map(|m| m.topic.as_str()).collect();
    topics.sort_unstable();
    topics.dedup();
    // Printed because Story 4.4's write-up quoted a distinct-topic count that
    // this instrument did not compute; the reader could not reproduce it.
    println!("  distinct topics    : {}", topics.len());

    if redeliveries > 0 {
        println!(
            "\n  {redeliveries} MESSAGE(S) CARRY THE DUP FLAG. Those are the broker redelivering,\n  \
             not the host publishing again. Do not count them as separate publishes."
        );
    }

    if seen.is_empty() {
        println!(
            "\n  NOTHING WAS RECEIVED. Before concluding the host publishes no STATE,\n  \
             rule out each of these and say in the write-up which you eliminated:\n    \
             1. topic filter — re-run with SMARTME_STATE_FILTER='#'\n    \
             2. wrong broker or port\n    \
             3. the host publishes on a non-standard topic shape\n    \
             4. the host publishes only on a state transition, and none occurred\n  \
             A broker ACL is NOT on this list: the SubAck was checked at subscribe\n  \
             time and did not refuse us. A quiet transcript is a finding only once\n  \
             the remaining four are excluded — and only 4 can be excluded by\n  \
             observing a transition, which is why the story requires a restart."
        );
    } else if live == 0 {
        println!(
            "\n  ONLY RETAINED MESSAGES. Everything here is a stored snapshot the broker\n  \
             replayed on subscribe — no transition was observed. An Ignition restart\n  \
             during the window would have produced retain=false messages.\n  \
             A snapshot describes the broker's memory, not the host's behaviour."
        );
    }
}

/// Read-only observation of the production broker's STATE traffic.
///
/// Ignored by default and gated on an env var with no default: this must never
/// run unattended, and never against a broker nobody chose.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "read-only observation of a real broker; needs SMARTME_STATE_BROKER"]
async fn observe_primary_host_state() {
    let target = std::env::var("SMARTME_STATE_BROKER")
        .expect("set SMARTME_STATE_BROKER=host:port — there is deliberately no default");
    let (host, port) = target
        .rsplit_once(':')
        .expect("SMARTME_STATE_BROKER must be host:port");
    let port: u16 = port.parse().expect("the port must be a number");

    let filter =
        std::env::var("SMARTME_STATE_FILTER").unwrap_or_else(|_| "spBv1.0/STATE/#".to_string());
    // A typo must NOT silently become the default. `900s` or `9OO` used to parse
    // as None and fall back to 90, closing the window thirteen minutes early in
    // a run built around a production restart that cannot be repeated cheaply.
    // Absent is a default; malformed is an error.
    let seconds: u64 = match std::env::var("SMARTME_STATE_SECONDS") {
        Ok(raw) => raw.trim().parse().unwrap_or_else(|_| {
            panic!(
                "SMARTME_STATE_SECONDS={raw:?} is not a whole number of seconds.\n  \
                 Refused rather than defaulted: a silent fallback to 90 would close the\n  \
                 observation window early and the run would look complete."
            )
        }),
        Err(_) => 90,
    };
    let window = Duration::from_secs(seconds);

    // Configurable because two observers must be able to watch different topic
    // shapes at once: a broker evicts the older session when a client id
    // reconnects, so a shared id would make them silently unplug each other —
    // the exact hazard `common/mod.rs:93-97` records for the chaos tests.
    let client_id = match std::env::var("SMARTME_STATE_CLIENT_ID") {
        Ok(id) => id,
        Err(_) => {
            eprintln!(
                "  NOTE: using the default client id. If a second observer is running,\n  \
                 set SMARTME_STATE_CLIENT_ID — a shared id makes two observers evict\n  \
                 each other in a reconnect loop, and neither transcript says so."
            );
            "state-observer-4-4".to_string()
        }
    };

    eprintln!(
        "observing {host}:{port} on {filter:?} as {client_id:?} for {seconds}s — publishing nothing"
    );

    let mut rx = observe(host, port, &filter, &client_id).await;
    let mut run = Run::default();
    let started = tokio::time::Instant::now();
    let deadline = started + window;
    // A first ConnAck is the connection itself, not a reconnect.
    let mut connections = 0usize;

    while tokio::time::Instant::now() < deadline {
        // saturating: the deadline can pass between the loop condition and here,
        // and `Instant - Instant` panics on a negative result.
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(Signal::Message(m))) => {
                eprintln!(
                    "  received {} bytes on {} (retain={}, qos={:?}, dup={})",
                    m.payload.len(),
                    m.topic,
                    m.retain,
                    m.qos,
                    m.dup
                );
                run.seen.push(*m);
            }
            Ok(Some(Signal::Connected)) => {
                connections += 1;
                if connections > 1 {
                    run.reconnects += 1;
                    eprintln!("  [reconnect #{}] a gap of unknown width", run.reconnects);
                }
            }
            Ok(Some(Signal::Granted(codes))) => {
                if let Some(SubscribeReasonCode::Success(qos)) = codes.first() {
                    if *qos != QoS::AtLeastOnce {
                        run.downgraded_to = Some(*qos);
                    }
                }
            }
            Ok(Some(Signal::Transport(error))) => run.transport_errors.push(error),
            Ok(Some(Signal::Ended(reason))) => {
                run.ended_early = Some(reason);
                break;
            }
            // The channel closed without an `Ended`: the observer task returned
            // or panicked. Distinguished from the window elapsing, because the
            // first version treated the two identically and then printed the
            // full nominal window over a truncated transcript.
            Ok(None) => {
                run.ended_early = Some(
                    "the observer task stopped without saying why (it returned or panicked)"
                        .to_string(),
                );
                break;
            }
            Err(_elapsed) => break,
        }
    }

    let observed_for = tokio::time::Instant::now().saturating_duration_since(started);
    report(&run, &filter, window, observed_for);

    // No assertion. The assertion is a human reading the transcript against the
    // runbook — the same shape as the Tier-3 contract test, and for the same
    // reason: there is nothing here we could assert that would not simply be our
    // own expectation checked against itself.
    //
    // The one thing worth failing on is the instrument lying about its own run:
    // a partial window reported as a complete one is how a false negative gets
    // written down as a finding.
    if let Some(reason) = run.ended_early {
        panic!(
            "the observation is INCOMPLETE and must not be recorded as a finding: {reason}\n  \
             The transcript above is printed in full — read it, but do not conclude\n  \
             anything from what is missing."
        );
    }
}
