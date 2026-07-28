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
//! - `SMARTME_STATE_SECONDS` — observation window, default 90.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
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
    retain: bool,
    qos: QoS,
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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is after the epoch")
        .as_millis()
}

/// Subscribes and streams raw messages. Never publishes.
async fn observe(host: &str, port: u16, filter: &str, client_id: &str) -> mpsc::Receiver<Observed> {
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
    let filter = filter.to_string();

    tokio::spawn(async move {
        let mut ready_tx = Some(ready_tx);
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    // QoS 1: the host publishes STATE at QoS 1, and subscribing
                    // lower would silently downgrade the delivery we are here to
                    // characterise.
                    client
                        .subscribe(filter.clone(), QoS::AtLeastOnce)
                        .await
                        .expect("subscribe");
                }
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    if let Some(tx) = ready_tx.take() {
                        let _ = tx.send(());
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
                    };
                    if tx.send(observed).await.is_err() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("  [transport] {error}");
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
    match tokio::time::timeout(CONNECT_DEADLINE, ready_rx).await {
        Ok(Ok(())) => {}
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
    }
    rx
}

/// Prints the transcript and the diagnostics a quiet run needs in order to mean
/// anything.
fn report(seen: &[Observed], filter: &str, window: Duration) {
    println!("\n=== TRANSCRIPT ({} message(s)) ===", seen.len());
    for (i, m) in seen.iter().enumerate() {
        println!(
            "\n[{i}] +{}ms  retain={}  qos={:?}\n     topic   : {}\n     host_id : {}\n     payload : {:?} ({} bytes)",
            m.received_at_ms - seen[0].received_at_ms,
            m.retain,
            m.qos,
            m.topic,
            m.last_token(),
            m.body(),
            m.payload.len()
        );
        match m.json_shape() {
            Some(shape) => println!("     json    : {{ {shape} }}"),
            None => println!(
                "     json    : NOT a JSON object — record this, it contradicts the specification"
            ),
        }
    }

    let retained = seen.iter().filter(|m| m.retain).count();
    let live = seen.len() - retained;
    println!("\n=== SUMMARY ===");
    println!("  filter            : {filter}");
    println!("  window            : {}s", window.as_secs());
    println!("  retained (snapshot): {retained}");
    println!("  live (published)   : {live}");
    let mut hosts: Vec<&str> = seen.iter().map(|m| m.last_token()).collect();
    hosts.sort_unstable();
    hosts.dedup();
    println!("  distinct host ids  : {hosts:?}");

    if seen.is_empty() {
        println!(
            "\n  NOTHING WAS RECEIVED. Before concluding the host publishes no STATE,\n  \
             rule out each of these and say in the write-up which you eliminated:\n    \
             1. topic filter — re-run with SMARTME_STATE_FILTER='#'\n    \
             2. broker ACL hiding the topic from this client id\n    \
             3. wrong broker or port\n    \
             4. the host publishes on a non-standard topic shape\n  \
             A quiet transcript is a finding only once all four are excluded."
        );
    } else if live == 0 {
        println!(
            "\n  ONLY RETAINED MESSAGES. Everything here is a stored snapshot the broker\n  \
             replayed on subscribe — no transition was observed. An Ignition restart\n  \
             during the window would have produced retain=false messages."
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
    let seconds: u64 = std::env::var("SMARTME_STATE_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90);
    let window = Duration::from_secs(seconds);

    // Configurable because two observers must be able to watch different topic
    // shapes at once: a broker evicts the older session when a client id
    // reconnects, so a shared id would make them silently unplug each other —
    // the exact hazard `common/mod.rs:93-97` records for the chaos tests.
    let client_id = std::env::var("SMARTME_STATE_CLIENT_ID")
        .unwrap_or_else(|_| "state-observer-4-4".to_string());

    eprintln!(
        "observing {host}:{port} on {filter:?} as {client_id:?} for {seconds}s — publishing nothing"
    );

    let mut rx = observe(host, port, &filter, &client_id).await;
    let mut seen = Vec::new();
    let deadline = tokio::time::Instant::now() + window;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(m)) => {
                eprintln!(
                    "  received {} bytes on {} (retain={}, qos={:?})",
                    m.payload.len(),
                    m.topic,
                    m.retain,
                    m.qos
                );
                seen.push(m);
            }
            Ok(None) => break,
            Err(_elapsed) => break,
        }
    }

    report(&seen, &filter, window);

    // No assertion. The assertion is a human reading the transcript against the
    // runbook — the same shape as the Tier-3 contract test, and for the same
    // reason: there is nothing here we could assert that would not simply be our
    // own expectation checked against itself.
}
