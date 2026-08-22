//! Observes what the bridge actually puts on the wire for the `Cause` property
//! ([#68]), so the question *"is it on the wire?"* stops being confused with
//! *"does Ignition display it?"*.
//!
//! # This publishes NOTHING
//!
//! It subscribes and prints. No will, no birth, no `publish` call, `clean_session`
//! set explicitly. The only broker available is production, with a live Ignition on
//! it — the same constraint story 4.4 worked under, and this file follows its
//! shape deliberately rather than inventing a second one.
//!
//! # Why it exists
//!
//! Contract v4 added a `Cause` property to every non-good metric. On 2026-08-12,
//! against contract v6 on the real deployment, the Tier-3 gate's step 4 showed no
//! `Cause` in the Designer while production tags showed one — and the operator
//! rightly said they were not sure they had looked in the right place. **An
//! observation nobody can locate is not a measurement.** This instrument answers
//! the half that belongs to us: what leaves the bridge. What a host does with it is
//! a separate question and stays [#68]'s.
//!
//! # Every way this instrument can lie, and what stops it
//!
//! - **`Cause` only exists on a NON-GOOD metric.** If every meter is healthy for
//!   the whole window, no property is owed and an empty result means nothing at
//!   all. This is the dangerous one, because silence reads as an answer. The
//!   summary therefore counts non-good metrics separately and refuses to conclude
//!   when that count is zero.
//! - **The broker denies the subscription.** MQTT 3.1.1 answers a refused SUBSCRIBE
//!   with return code `0x80`, not an error. The codes are printed and a denial
//!   fails the run, rather than looking like a quiet topic.
//! - **The filter names a topic nobody publishes.** Every topic seen is printed, so
//!   a filter aimed at the wrong group shows as an empty transcript WITH the filter
//!   quoted beside it.
//! - **A payload fails to decode and is dropped.** `common::named_subscriber_on`
//!   drops those silently (`if let Ok(payload) = decode(..)` with no `else`), which
//!   is the false-negative shape that produced the contract-v1 quality codes. Here
//!   they are counted and reported.
//! - **The window is shorter than the publish period.** The bridge publishes every
//!   30 s by default, so a 10 s window can see nothing from a healthy fleet. The
//!   default is 90 s and the chosen window is printed.
//! - **A DBIRTH is not observed at all** unless the bridge reconnects or is asked to
//!   rebirth during the window. Its absence is reported as *not observed*, never as
//!   *carries no cause* — those are different findings and only one of them is
//!   evidence.
//!
//! # Running it
//!
//! ```bash
//! SMARTME_OBSERVE_BROKER=192.168.1.30:1883 \
//!   cargo test -p smartme-bridge --test observe_cause_property -- --ignored --nocapture
//! ```
//!
//! `SMARTME_OBSERVE_FILTER` defaults to `spBv1.0/#`; `SMARTME_OBSERVE_SECONDS` to 90.
//!
//! [#68]: https://github.com/guycorbaz/smartme_mqtt/issues/68

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use smartme_bridge::adapters::sparkplug_publisher::ignition_quality_code;
use smartme_bridge::domain::Quality;
use std::time::Duration;
use tokio::sync::mpsc;

/// What the observer task hands back. Distinguishing these is what lets the
/// summary say *why* it saw nothing.
enum Signal {
    /// The SubAck arrived. The return codes travel to the caller through the
    /// oneshot rather than here, so this variant carries nothing — a second copy
    /// would be a field nobody reads, which `clippy -D warnings` rightly refuses.
    Granted,
    Message {
        topic: String,
        payload: Vec<u8>,
    },
    /// A transport error, already printed by the task that saw it.
    Transport,
}

/// One metric, as it arrived.
struct MetricSeen {
    name: String,
    is_null: bool,
    quality: Option<u32>,
    properties: Vec<(String, String)>,
    /// The metric's value when it is a string — which is what a cause metric is.
    string_value: Option<String>,
}

/// The cause published for the measurement metric `name`, read from its SIBLING
/// metric in the same payload.
///
/// **It read a `Cause` property until contract v12.** The cause is a metric now
/// (ADR 0044): a property is written by a BIRTH and never updated by a DDATA,
/// which this very instrument helped settle on 2026-08-22 by reading the wire
/// while an operator watched the screen. A metric named `Cause/Power` is what a
/// host renders as a tag whose value moves.
///
/// A cause metric is not itself a measurement, so it never counts as one: the
/// counters below skip any metric whose name starts with `Cause/`.
fn cause_for<'a>(metrics: &'a [MetricSeen], name: &str) -> Option<&'a str> {
    let wanted = format!("Cause/{name}");
    metrics
        .iter()
        .find(|m| m.name == wanted)
        .and_then(|m| m.string_value.as_deref())
}

impl MetricSeen {
    fn is_a_cause(&self) -> bool {
        self.name.starts_with("Cause/")
    }

    /// Ignition's encoding, read back. `None` when the metric carries no quality
    /// property at all — which is itself worth seeing.
    fn is_good(&self) -> Option<bool> {
        self.quality
            .map(|code| code == ignition_quality_code(Quality::Good))
    }
}

fn quality_label(code: u32) -> String {
    if code == ignition_quality_code(Quality::Good) {
        format!("{code} (Good)")
    } else if code == ignition_quality_code(Quality::Stale) {
        format!("{code} (Bad_Stale)")
    } else if code == ignition_quality_code(Quality::Bad) {
        format!("{code} (Bad)")
    } else {
        format!("{code} (UNKNOWN — not one of this bridge's three codes)")
    }
}

/// Pulls every metric out of a decoded payload, properties included.
fn metrics_of(payload: &sparkplug_b::protobuf::Payload) -> Vec<MetricSeen> {
    payload
        .metrics
        .iter()
        .map(|m| {
            let mut quality = None;
            let mut properties = Vec::new();
            if let Some(props) = m.properties.as_ref() {
                for (key, value) in props.keys.iter().zip(props.values.iter()) {
                    // The value is a oneof; render whichever arm is set rather
                    // than assuming a string, so a property encoded as an int is
                    // seen rather than silently skipped.
                    let rendered = match &value.value {
                        Some(
                            sparkplug_b::protobuf::payload::property_value::Value::StringValue(s),
                        ) => s.clone(),
                        Some(sparkplug_b::protobuf::payload::property_value::Value::IntValue(
                            i,
                        )) => i.to_string(),
                        Some(other) => format!("{other:?}"),
                        None => "<no value>".to_string(),
                    };
                    if key == "Quality" {
                        if let Some(
                            sparkplug_b::protobuf::payload::property_value::Value::IntValue(i),
                        ) = &value.value
                        {
                            quality = Some(*i);
                        }
                    }
                    properties.push((key.clone(), rendered));
                }
            }
            MetricSeen {
                name: m.name.clone().unwrap_or_else(|| "<unnamed>".to_string()),
                is_null: m.value.is_none(),
                quality,
                properties,
                string_value: match &m.value {
                    Some(sparkplug_b::protobuf::payload::metric::Value::StringValue(v)) => {
                        Some(v.clone())
                    }
                    _ => None,
                },
            }
        })
        .collect()
}

async fn observe(host: &str, port: u16, filter: &str, client_id: &str) -> mpsc::Receiver<Signal> {
    let mut options = MqttOptions::new(client_id, host, port);
    options.set_keep_alive(Duration::from_secs(10));
    // Explicit, not inherited — #35 records that relying on a library default is
    // how a MUST goes unasserted, and a persistent session on a production broker
    // would queue messages for a client id that never returns.
    options.set_clean_session(true);

    let (client, mut eventloop) = AsyncClient::new(options, 256);
    let (tx, rx) = mpsc::channel(1024);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task_filter = filter.to_string();

    tokio::spawn(async move {
        let mut ready_tx = Some(ready_tx);
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    // AtMostOnce, because that is what the norm mandates for
                    // DDATA (`tck-id-payloads-ddata-qos`) and therefore what a
                    // real consumer receives. Subscribing higher would not
                    // upgrade delivery, but it would misdescribe the contract
                    // this file exists to characterise.
                    if let Err(error) = client.subscribe(task_filter.clone(), QoS::AtMostOnce).await
                    {
                        eprintln!("  [transport] SUBSCRIBE could not be sent: {error}");
                        return;
                    }
                }
                Ok(Event::Incoming(Packet::SubAck(ack))) => {
                    if tx.send(Signal::Granted).await.is_err() {
                        return;
                    }
                    if let Some(tx) = ready_tx.take() {
                        let _ = tx.send(ack.return_codes.clone());
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    if tx
                        .send(Signal::Message {
                            topic: p.topic.clone(),
                            payload: p.payload.to_vec(),
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("  [transport] {error}");
                    let _ = tx.send(Signal::Transport).await;
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });

    const CONNECT_DEADLINE: Duration = Duration::from_secs(15);
    let granted = match tokio::time::timeout(CONNECT_DEADLINE, ready_rx).await {
        Ok(Ok(codes)) => codes,
        Ok(Err(_)) => {
            panic!("the observer task ended before it subscribed — see [transport] above")
        }
        Err(_) => panic!(
            "no SubAck within {}s. The broker did not accept a subscription.\n  \
             1. is SMARTME_OBSERVE_BROKER reachable from here (`nc -vz host port`)?\n  \
             2. is a sandbox or firewall in the way?\n  \
             3. does the broker require credentials this observer does not send?\n  \
             4. does an ACL forbid this client id?",
            CONNECT_DEADLINE.as_secs()
        ),
    };
    let denied: Vec<_> = granted
        .iter()
        .filter(|c| matches!(c, rumqttc::SubscribeReasonCode::Failure))
        .collect();
    assert!(
        denied.is_empty(),
        "the broker REFUSED the subscription to {filter:?} (return codes {granted:?}). \
         An empty transcript would have looked exactly like a quiet topic."
    );
    println!("  [subscribed] {filter:?} granted {granted:?}");

    rx
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "read-only observation of a real broker; needs SMARTME_OBSERVE_BROKER"]
async fn observe_cause_property() {
    let target = std::env::var("SMARTME_OBSERVE_BROKER").expect(
        "set SMARTME_OBSERVE_BROKER=host:port. This test reads a REAL broker and publishes nothing.",
    );
    let (host, port) = target
        .rsplit_once(':')
        .map(|(h, p)| (h.to_string(), p.parse::<u16>().expect("port")))
        .unwrap_or((target.clone(), 1883));
    let filter =
        std::env::var("SMARTME_OBSERVE_FILTER").unwrap_or_else(|_| "spBv1.0/#".to_string());
    let seconds: u64 = std::env::var("SMARTME_OBSERVE_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90);
    let client_id = format!("smartme-observe-cause-{}", std::process::id());

    println!("\n  Observing {host}:{port}, filter {filter:?}, for {seconds}s.");
    println!("  This publishes NOTHING. Client id {client_id:?}.");
    println!(
        "  The bridge's default publish period is 30s, so a healthy fleet shows \
         roughly {} update(s) per meter.\n",
        seconds / 30
    );

    let mut rx = observe(&host, port, &filter, &client_id).await;

    let (mut decoded, mut undecodable, mut non_good, mut non_good_with_cause) = (0, 0, 0, 0);
    let mut births_seen = 0usize;
    let mut births_declaring_cause = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);

    while let Ok(Some(signal)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        let (topic, payload) = match signal {
            Signal::Message { topic, payload } => (topic, payload),
            Signal::Granted | Signal::Transport => continue,
        };
        let Ok(parsed) = sparkplug_b::decode(&payload) else {
            undecodable += 1;
            // NAME THE EXPECTED CASE, so an operator does not spend the evening on
            // it. `spBv1.0/STATE/…` is a Host Application's birth/death
            // certificate and is JSON, not protobuf — story 4.4 characterised it,
            // and the default filter here is broad enough to catch it. Anything
            // else failing to decode is a genuine finding.
            let expected = topic.contains("/STATE/");
            println!(
                "  [undecodable] {topic} ({} bytes){}",
                payload.len(),
                if expected {
                    " — EXPECTED: a Host Application STATE payload is JSON, not protobuf (story 4.4)"
                } else {
                    " — UNEXPECTED on a Sparkplug topic; worth an issue"
                }
            );
            continue;
        };
        decoded += 1;
        let is_birth = topic.contains("/DBIRTH/") || topic.contains("/NBIRTH/");
        if is_birth {
            births_seen += 1;
        }

        let metrics = metrics_of(&parsed);
        println!("  ── {topic}");
        for m in &metrics {
            let quality = m
                .quality
                .map(quality_label)
                .unwrap_or_else(|| "<no Quality property>".to_string());
            let value = if m.is_null { "null" } else { "a value" };
            println!("     {:<20} {value:<8} quality {quality}", m.name);
            for (k, v) in &m.properties {
                if k != "Quality" {
                    println!("       · {k} = {v}");
                }
            }
            // The counters that decide what this run may conclude. A cause metric
            // is not a measurement and must not be counted as one — it is Good by
            // design, so it would never reach this branch, but saying so keeps the
            // next reader from wondering.
            if !m.is_a_cause() && m.is_good() == Some(false) {
                non_good += 1;
                let has_cause = cause_for(&metrics, &m.name).is_some();
                if has_cause {
                    non_good_with_cause += 1;
                }
                if is_birth && has_cause {
                    births_declaring_cause += 1;
                }
            }
        }
    }

    println!("\n  ───────────────────────── summary ─────────────────────────");
    println!("  Sparkplug messages decoded : {decoded}");
    println!("  payloads that did NOT decode: {undecodable}");
    println!("  BIRTH messages observed     : {births_seen}");
    println!("  NON-GOOD metrics observed   : {non_good}");
    println!("  …of those, carrying a Cause : {non_good_with_cause}");

    if non_good == 0 {
        println!(
            "\n  INCONCLUSIVE — and this is a real answer, not a failure.\n  \
             Every metric observed was GOOD, so no `Cause` was owed and its absence\n  \
             proves nothing. `Cause` appears only on a metric the bridge refused.\n  \
             Re-run while a meter is degraded: unplug one, or wait for the cloud to\n  \
             go quiet, and watch the same window again."
        );
    } else if non_good_with_cause == non_good {
        println!(
            "\n  THE PROPERTY IS ON THE WIRE. Every one of the {non_good} non-good metric(s)\n  \
             carried its `Cause`. What remains open is what a HOST does with it,\n  \
             which is [#68] and is not answerable from here."
        );
    } else {
        println!(
            "\n  ⚠ A NON-GOOD METRIC REACHED THE WIRE WITHOUT ITS CAUSE — {} of {non_good}.\n  \
             That contradicts the invariant contract v4 was struck for, and the\n  \
             known exception is the cold-start DBIRTH, which goes through\n  \
             `cold_start_metrics` and attaches no property. Check the topics above:\n  \
             if the bare ones are BIRTHs, this is that known gap; if they are DDATA,\n  \
             it is a defect and worth an issue.",
            non_good - non_good_with_cause
        );
    }
    if births_seen == 0 {
        println!(
            "\n  NO BIRTH WAS OBSERVED in this window, so this run says NOTHING about\n  \
             whether a DBIRTH declares `Cause` — which is the half of [#68] that\n  \
             decides whether a host can ever show it. Not observed is not absent."
        );
    } else {
        println!(
            "  BIRTHs declaring a Cause    : {births_declaring_cause} of {births_seen} observed"
        );
    }
}
