//! The mqtt driver task (Story 1.12).
//!
//! It owns the session lifecycle and `bdSeq` persistence, and decides NO truth:
//! it transports the verdict the poll task already reached.
//!
//! The boot order is not negotiable, and it is the reason the will is built
//! before anything connects:
//!
//! 1. restore `bdSeq` from disk and open the next session,
//! 2. serialise the DEATH certificate for that session,
//! 3. register it as the connection's last will IN the CONNECT packet,
//! 4. connect,
//! 5. subscribe to the node's NCMD topic,
//! 6. publish the BIRTH.
//!
//! Get this wrong and the broker holds no will while the node is alive — the
//! node dies and nobody is told, which is the silent lie this project exists to
//! prevent.
//!
//! # Why step 5 precedes step 6 (Story 4.6)
//!
//! `tck-id-message-flow-edge-node-ncmd-subscribe` and the section preamble that
//! introduces it (`Sparkplug_5_Operational_Behavior.adoc:155-163`) are explicit:
//! *"**Prior to sending an NBIRTH message**, the MQTT client associated with the
//! Edge Node must subscribe to receive NCMD messages"*, and *"It MUST subscribe
//! on this topic with a QoS of 1."* Same sequence is not enough — a host that
//! receives an NBIRTH may answer it immediately with a rebirth request, and a
//! node that has not yet subscribed never sees it.
//!
//! Steps 5 and 6 both live in the `Transport::Connected` arm, which fires on
//! EVERY CONNACK, so the ordering holds on a reconnect as much as on the first
//! connect.
//!
//! Re-subscribing on every connect is correct whether or not the broker kept
//! the old subscription, and it is written that way ON PURPOSE. The tempting
//! justification — *"`rumqttc` connects with a clean session, so the broker
//! discards the subscription"* — is true of the dependency's current default and
//! is exactly what this project's own conformance matrix records as a **gap
//! (unproven)** under `tck-id-principles-persistence-clean-session-311`:
//! `set_clean_session` is never called and no test asserts the flag
//! ([#35](https://github.com/guycorbaz/smartme_mqtt/issues/35), Story 4.10).
//! Making the re-subscribe *depend* on that premise would convert an open gap
//! into a settled fact. It does not depend on it: an unnecessary SUBSCRIBE costs
//! one packet, and a missing one costs every command for the life of the
//! session.
//!
//! The driver does NOT wait for the SubAck before birthing. MQTT delivers one
//! connection's packets in order, so a SUBSCRIBE queued before the PUBLISH
//! satisfies the clause; awaiting the acknowledgement would delay every birth on
//! an unknown latency and could hang a session that is otherwise healthy. The
//! SubAck is checked when it arrives, not waited on.
//!
//! # Why the EventLoop gets its own task
//!
//! `EventLoop::poll` is NOT cancellation-safe: dropping it mid-poll can abandon
//! a half-finished CONNECT (after the broker has already registered the will,
//! which then fires against a node that never birthed) or a half-written
//! packet. So it is never a `select!` branch — it runs alone in its own task and
//! reports what it sees through a channel.
//!
//! # Session identity, and a recorded deviation
//!
//! `rumqttc` reconnects internally and rebuilds the CONNECT packet from the
//! `MqttOptions` captured at construction — so the registered will can never be
//! updated. The session number is therefore FIXED for the lifetime of one
//! client: a reconnect re-births under the same `bdSeq`, which is exactly the
//! number the will carries. The Sparkplug specification would have it increment
//! per CONNECT; advancing it here would leave the broker holding a death
//! certificate for a session that no longer exists, and a consumer that pairs
//! death to birth by `bdSeq` would IGNORE the death and keep showing a frozen
//! value as live. Matching the will is the honest half of that trade; owning
//! the reconnect loop (and rebuilding the client per CONNECT) is deferred.

use std::path::PathBuf;
use std::time::Duration;

use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS, SubscribeReasonCode};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::adapters::sparkplug_publisher::{Outbound, Published, Sink, SparkplugPublisher};
use crate::core::channel::MeterUpdate;
use crate::core::clock::Clock;
use crate::domain::Serial;
use sparkplug_b::{BdSeq, MessageType};

/// How to reach the broker and how to behave on it.
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// MQTT client identifier.
    pub client_id: String,
    /// Broker host.
    pub host: String,
    /// Broker port.
    pub port: u16,
    /// Keep-alive interval.
    pub keep_alive: Duration,
    /// Where the session number is persisted across restarts.
    pub bd_seq_path: PathBuf,
    /// Bound of the outbound queue handed to the client.
    pub capacity: usize,
    /// How long to keep pumping the transport after the DEATH is queued, so it
    /// actually reaches the wire before the socket closes.
    pub death_flush: Duration,
}

/// The persisted session number.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedBdSeq {
    bd_seq: u8,
}

/// Reads the last session number, or the before-first sentinel.
pub fn load_bd_seq(path: &std::path::Path) -> BdSeq {
    match crate::persist::load::<PersistedBdSeq>(path) {
        Ok(persisted) => BdSeq::new(persisted.bd_seq),
        Err(error) => {
            // Missing or corrupt. Starting from the sentinel replays numbers a
            // long-lived consumer may have seen; refusing to start would be
            // safer still, and Epic 3's config validation is where that belongs.
            tracing::warn!(%error, "no readable bdSeq state; starting from the sentinel");
            BdSeq::before_first()
        }
    }
}

/// Persists the session number BEFORE connecting, so a crash between connect
/// and the next boot cannot replay it.
pub fn store_bd_seq(path: &std::path::Path, bd_seq: BdSeq) -> std::io::Result<()> {
    crate::persist::persist_atomic(
        path,
        &PersistedBdSeq {
            bd_seq: bd_seq.value(),
        },
    )
}

/// A sink that queues outbound messages for the driver to publish.
#[derive(Debug, Default)]
struct Queue {
    pending: Vec<Outbound>,
}

impl Sink for Queue {
    fn emit(&mut self, message: Outbound) {
        self.pending.push(message);
    }
}

/// Delivery semantics per message type.
///
/// The Sparkplug specification requires QoS 0 and retain false for every
/// edge-node message. Retain is the important half here: a retained payload is
/// a value the broker replays to a new subscriber with no way for it to know
/// how old the value is — a stored lie. Freshness is the BIRTH's job, not the
/// broker's.
fn qos_for(_message: MessageType) -> (QoS, bool) {
    (QoS::AtMostOnce, false)
}

/// What the transport task saw.
///
/// Deliberately no longer `Copy`: a SubAck carries the broker's answer, which is
/// a `Vec`. Losing `Copy` is the price of not throwing that answer away.
#[derive(Debug, PartialEq, Eq)]
enum Transport {
    /// The broker accepted the connection: it is time to subscribe, then birth.
    Connected,
    /// The connection dropped; the will covers us until we reconnect.
    Lost,
    /// The broker answered our SUBSCRIBE. Carried through rather than discarded:
    /// a refusal is return code `0x80` — not an error, not a disconnect — so a
    /// discarded answer makes a refused subscription indistinguishable from a
    /// topic nobody publishes on.
    Subscribed(Vec<SubscribeReasonCode>),
}

/// One inbound node command, exactly as it arrived.
///
/// Bytes, not a decoded payload: the decode can fail, and a malformed payload
/// from the network is an expected input whose failure belongs in a trace, not
/// in the transport task.
#[derive(Debug, PartialEq, Eq)]
struct Command {
    topic: String,
    payload: Vec<u8>,
}

/// Bound of the inbound-command queue.
///
/// Small on purpose, and paired with a traced drop rather than a block — see
/// [`pump_transport`]. Unbounded would trade a stall for unbounded memory.
const COMMAND_QUEUE: usize = 8;

/// What the broker granted, distilled from the SubAck return codes.
///
/// Split out from the trace so it can be tested: the three failing shapes are
/// exactly the ones a live broker produces least often and a review notices
/// least easily.
#[derive(Debug, PartialEq, Eq)]
enum Granted {
    /// QoS 1, as `tck-id-message-flow-edge-node-ncmd-subscribe` requires.
    AsRequired,
    /// Accepted, but not at the mandated QoS 1. A downgrade to 0 is silent on
    /// every other channel: only this byte says so.
    NotAsRequired(QoS),
    /// Refused outright — return code `0x80`.
    Refused,
    /// A SubAck with no return code at all: a malformed answer, and not the
    /// same thing as a grant.
    Empty,
    /// More return codes than this bridge issued topic filters. The answer
    /// cannot be attributed, and guessing which entry is ours would put a
    /// confident, possibly wrong line in the log about the exact byte this
    /// story exists to stop discarding.
    Unattributable(usize),
}

/// Reads the broker's answer.
///
/// # Why the whole slice is examined and not just the first entry
///
/// A SubAck carries one return code per topic filter in the SUBSCRIBE it
/// answers, in order. This driver issues exactly one filter per connect, so a
/// well-formed answer holds exactly one code — and that invariant is CHECKED
/// here rather than assumed, because the code that assumes it goes wrong
/// silently.
///
/// It is not a hypothetical: Story 4.5 must subscribe to a STATE topic
/// (`tck-id-…-state-subs`), and `pump_transport` forwards every SubAck without
/// correlating it to the SUBSCRIBE it answers — `try_subscribe` returns no
/// packet id to correlate with. On the day a second subscription exists, taking
/// `codes[0]` would report a refused STATE subscription as a refused *NCMD*
/// one, sending the operator to the wrong ACL. `Unattributable` says "I do not
/// know" instead, which is the only honest answer available without packet-id
/// correlation.
fn granted(codes: &[SubscribeReasonCode]) -> Granted {
    match codes {
        [] => Granted::Empty,
        [SubscribeReasonCode::Failure] => Granted::Refused,
        [SubscribeReasonCode::Success(QoS::AtLeastOnce)] => Granted::AsRequired,
        [SubscribeReasonCode::Success(qos)] => Granted::NotAsRequired(*qos),
        many => Granted::Unattributable(many.len()),
    }
}

/// How an inbound command payload was understood.
///
/// The bridge recognises NO command yet — `Node Control/Rebirth` is Story 4.7 —
/// so every well-formed payload lands in `Unrecognised` and is dropped. Nothing
/// here is a quality or staleness verdict: this classifies bytes for a log line,
/// and the driver still decides no truth.
#[derive(Debug, PartialEq, Eq)]
enum Inbound {
    /// The bytes are not a Sparkplug payload. Expected input, not a bug.
    Undecodable(String),
    /// It decoded and carried nothing to act on.
    NoMetrics,
    /// It decoded and named metrics, none of which this bridge implements.
    Unrecognised(Vec<String>),
}

/// How a metric identified itself, for the trace.
///
/// Sparkplug lets a host address a metric by `alias` INSTEAD of by name — the
/// alias is established in the BIRTH and used thereafter. This bridge publishes
/// `alias: None` on everything it sends, but that governs what it emits, not
/// what a host may send it. An alias-addressed `Node Control/Rebirth` is legal
/// and would arrive with no name at all.
///
/// Collapsing that to a bare `<unnamed>` would reproduce the confusion the
/// `NoMetrics` arm exists to prevent: a name list whose names were, literally,
/// lost. Naming the alias keeps the trace diagnostic, and tells whoever
/// implements Story 4.7 that matching on the name alone is not sufficient.
fn metric_label(metric: &sparkplug_b::protobuf::payload::Metric) -> String {
    match (&metric.name, metric.alias) {
        (Some(name), _) => name.clone(),
        (None, Some(alias)) => format!("<alias {alias}>"),
        (None, None) => "<neither name nor alias>".to_string(),
    }
}

/// Classifies an inbound command payload for the trace.
///
/// Never panics: `decode` returns a `Result` and it is matched, because a
/// malformed payload arriving from the network is an ordinary event that must
/// not take the bridge down.
fn classify(payload: &[u8]) -> Inbound {
    match sparkplug_b::decode(payload) {
        Err(error) => Inbound::Undecodable(error.to_string()),
        Ok(decoded) if decoded.metrics.is_empty() => Inbound::NoMetrics,
        Ok(decoded) => Inbound::Unrecognised(decoded.metrics.iter().map(metric_label).collect()),
    }
}

/// Traces what the broker answered to the command subscription.
///
/// # Why this is a function and not four arms inside the `select!`
///
/// It was four arms, and the Story 4.6 review showed why that is not enough.
/// The falsification for AC2 collapsed [`granted`], which the unit test calls
/// directly — so it went red. But swapping the *bodies* of two arms, so that a
/// refusal logs *"granted at QoS 1"*, left every test green: nothing asserted
/// the log line, only the classification behind it. AC2 is written entirely in
/// terms of what an operator can see, so the line IS the behaviour, and a
/// behaviour inside a `select!` arm cannot be tested without running the whole
/// driver against a broker.
///
/// None of these outcomes aborts the session. A bridge that can publish but not
/// receive is strictly better than one that does neither, and the operator
/// learns which it is from the log alone — no broker access needed.
fn trace_subscription_outcome(topic: &str, outcome: Granted) {
    match outcome {
        Granted::AsRequired => {
            tracing::info!(%topic, "command subscription granted at QoS 1");
        }
        Granted::NotAsRequired(qos) => {
            tracing::warn!(
                %topic,
                ?qos,
                "the broker granted the command subscription at a QoS the \
                 specification does not permit (it requires 1); a command may be \
                 lost without either side noticing"
            );
        }
        Granted::Refused => {
            tracing::error!(
                %topic,
                "the broker REFUSED the command subscription (return code 0x80); \
                 the bridge keeps publishing but can receive no command — check \
                 the broker's ACL for this topic"
            );
        }
        Granted::Empty => {
            tracing::error!(
                %topic,
                "the broker answered the subscription with no return code; whether \
                 it is in force is unknown"
            );
        }
        Granted::Unattributable(count) => {
            tracing::error!(
                %topic,
                count,
                "a SubAck carried more return codes than this bridge subscribed to \
                 topics; the answer cannot be attributed to any one subscription \
                 and is NOT being read as a grant"
            );
        }
    }
}

/// Traces what arrived on the command topic, and drops it.
///
/// Extracted for the same reason as [`trace_subscription_outcome`]: AC3 says an
/// unrecognised command is ignored *loudly*, so the loudness is the property and
/// it needs a test that is not a broker.
fn trace_command_outcome(topic: &str, inbound: Inbound) {
    match inbound {
        Inbound::Undecodable(error) => {
            tracing::warn!(
                %topic,
                %error,
                "an NCMD that is not a Sparkplug payload was ignored"
            );
        }
        Inbound::NoMetrics => {
            tracing::info!(
                %topic,
                "an NCMD carrying no metric was ignored (never silently)"
            );
        }
        Inbound::Unrecognised(names) => {
            // The NAMES, not the payload: a name list is diagnostic, a full dump
            // is noise and may carry values.
            tracing::info!(
                %topic,
                ?names,
                "unrecognised NCMD ignored; this bridge implements no command yet"
            );
        }
    }
}

/// Runs the driver until the inbox closes or shutdown is signalled.
pub async fn run(
    config: MqttConfig,
    node: sparkplug_b::EdgeNode,
    meters: Vec<Serial>,
    clock: std::sync::Arc<dyn Clock + Send + Sync>,
    mut inbox: mpsc::Receiver<MeterUpdate>,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    // The command topic, built from the same validated grammar as everything we
    // publish, and built BEFORE `node` is handed to the publisher.
    //
    // The identifiers were already validated when the `EdgeNode` was
    // constructed and NCMD is node-level, so this cannot fail today. It is
    // still handled rather than unwrapped: publishing without a command path is
    // strictly better than not publishing.
    let ncmd_topic = match node.node_topic(MessageType::NCmd) {
        Ok(topic) => Some(topic),
        Err(error) => {
            tracing::error!(
                %error,
                "no NCMD topic could be built; the bridge will publish but can \
                 receive no command"
            );
            None
        }
    };

    // 1. Restore and advance the session number, persisting BEFORE connecting.
    let previous = load_bd_seq(&config.bd_seq_path);
    let mut publisher = SparkplugPublisher::new(node, previous);
    if let Err(error) = store_bd_seq(&config.bd_seq_path, publisher.bd_seq()) {
        tracing::error!(%error, "could not persist bdSeq; a restart may replay a session");
    }

    // 2. Serialise the DEATH for this session...
    let will = publisher.will(clock.wall());

    // 3. ...and register it IN the CONNECT packet, before connecting.
    let mut options = MqttOptions::new(&config.client_id, &config.host, config.port);
    options.set_keep_alive(config.keep_alive);
    let (qos, retain) = qos_for(MessageType::NDeath);
    options.set_last_will(rumqttc::LastWill::new(
        will.topic.clone(),
        will.payload.clone(),
        qos,
        retain,
    ));

    // 4. Connect — the EventLoop pumps in its own task (see the module docs).
    let (client, eventloop) = AsyncClient::new(options, config.capacity);
    let (transport_tx, mut transport_rx) = mpsc::channel(8);
    // Inbound commands get their OWN channel. Sharing the transport channel
    // would put a live, externally-driven path behind an 8-slot bound whose
    // sender blocks — see `pump_transport`.
    let (command_tx, mut command_rx) = mpsc::channel(COMMAND_QUEUE);
    let pump = tokio::spawn(pump_transport(
        eventloop,
        transport_tx,
        command_tx,
        ncmd_topic.clone(),
    ));

    loop {
        tokio::select! {
            event = transport_rx.recv() => {
                match event {
                    Some(Transport::Connected) => {
                        // 5. Connected: subscribe to NCMD BEFORE birthing. The
                        // order of these two statements IS the requirement; see
                        // the module docs.
                        if let Some(topic) = &ncmd_topic {
                            subscribe_to_commands(&client, topic);
                        }
                        // 6. Then publish the BIRTH.
                        let mut queue = Queue::default();
                        match publisher.birth(clock.wall(), &meters, &mut queue) {
                            Ok(()) => {
                                for message in queue.pending.drain(..) {
                                    publish(&client, message);
                                }
                                tracing::info!(bd_seq = publisher.bd_seq().value(), "session born");
                            }
                            Err(error) => {
                                tracing::error!(%error, "refusing to birth: nothing was published");
                            }
                        }
                    }
                    Some(Transport::Subscribed(codes)) => {
                        let topic = ncmd_topic.as_deref().unwrap_or("<none>");
                        trace_subscription_outcome(topic, granted(&codes));
                    }
                    Some(Transport::Lost) => {
                        tracing::warn!("transport lost; the will covers us until we reconnect");
                    }
                    None => {
                        tracing::error!("transport task ended; stopping");
                        break;
                    }
                }
            }
            command = command_rx.recv() => {
                let Some(command) = command else {
                    // The pump owns both senders, so this means the same thing
                    // as the transport channel closing. Breaking here rather
                    // than continuing matters: `recv` on a closed channel
                    // returns `None` immediately and forever, so a `continue`
                    // would spin.
                    tracing::error!("transport task ended; stopping");
                    break;
                };
                // Nothing is applied, so nothing can be applied by halves: this
                // story builds the plumbing and Story 4.7 gives it meaning.
                trace_command_outcome(&command.topic, classify(&command.payload));
            }
            update = inbox.recv() => {
                let Some(update) = update else {
                    tracing::info!("poll task closed the channel; stopping");
                    break;
                };
                let mut queue = Queue::default();
                match publisher.publish(&update, &mut queue) {
                    Ok(Published::Emitted) => {
                        for message in queue.pending.drain(..) {
                            publish(&client, message);
                        }
                    }
                    Ok(outcome) => {
                        // A traced drop, never silence.
                        tracing::warn!(
                            serial = %update.measurement.serial,
                            ?outcome,
                            "reading dropped"
                        );
                    }
                    Err(error) => {
                        tracing::error!(
                            serial = %update.measurement.serial,
                            %error,
                            "unpublishable reading dropped"
                        );
                    }
                }
            }
            _ = &mut shutdown => {
                tracing::info!("shutdown requested");
                break;
            }
        }
    }

    // A polite death: queue it, then keep the transport pumping long enough for
    // it to actually reach the wire. We never send a graceful DISCONNECT — that
    // instructs the broker to DISCARD the will, so if our explicit death did not
    // make it, nothing would ever be delivered. Dropping the connection instead
    // keeps the will as the second mechanism.
    publish(&client, publisher.will(clock.wall()));
    let flushed = tokio::time::timeout(config.death_flush, async {
        // The pump is still running; give it time to drain the request channel.
        tokio::time::sleep(config.death_flush).await;
    })
    .await;
    if flushed.is_err() {
        tracing::warn!("death flush timed out; falling back to the will");
    }
    pump.abort();
    tracing::info!("death published; transport dropped");
}

/// Owns the `EventLoop` alone, because `poll()` is not cancellation-safe.
///
/// `ncmd_topic` is what an inbound publish is matched against — exact equality,
/// not a prefix: the bridge subscribes to one topic and anything else arriving
/// here is not a command addressed to this node.
async fn pump_transport(
    mut eventloop: EventLoop,
    events: mpsc::Sender<Transport>,
    commands: mpsc::Sender<Command>,
    ncmd_topic: Option<String>,
) {
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                if events.send(Transport::Connected).await.is_err() {
                    return;
                }
            }
            Ok(Event::Incoming(Packet::SubAck(ack))) => {
                if events
                    .send(Transport::Subscribed(ack.return_codes))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Ok(Event::Incoming(Packet::Publish(publish)))
                if ncmd_topic.as_deref() == Some(publish.topic.as_str()) =>
            {
                // `try_send`, never `send().await`. THIS task is what answers
                // PINGREQ: blocking it on a full queue would cost the session
                // its keep-alive and the broker would disconnect a bridge that
                // was otherwise healthy. Same rule as `publish()` — a full
                // queue is a traced drop, never a block.
                let command = Command {
                    topic: publish.topic.clone(),
                    payload: publish.payload.to_vec(),
                };
                match commands.try_send(command) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(dropped)) => {
                        tracing::warn!(
                            topic = %dropped.topic,
                            "command queue full; NCMD dropped (never silently)"
                        );
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => return,
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "transport error");
                if events.send(Transport::Lost).await.is_err() {
                    return;
                }
                // rumqttc has no internal backoff: this sleep IS the backoff.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Queues the node's command subscription.
///
/// Never blocks and never aborts the session: a failure here costs the bridge
/// its command path, not its ability to publish, and the birth still goes out.
///
/// # The QoS here does not contradict `qos_for`
///
/// `qos_for` returns QoS 0 for every message, and
/// `every_edge_node_message_is_qos_zero_and_never_retained` pins it. That rule
/// governs what the edge node PUBLISHES. This is a *subscribe* QoS — a different
/// field, in a different packet, travelling the other way — and
/// `tck-id-message-flow-edge-node-ncmd-subscribe` mandates 1 for it. No conflict.
fn subscribe_to_commands(client: &AsyncClient, topic: &str) {
    // `try_subscribe` and `try_publish` feed the SAME request channel, so the
    // SUBSCRIBE queued here leaves the socket before the BIRTH queued after it.
    // That FIFO is what makes "prior to sending an NBIRTH" true without waiting
    // for the SubAck.
    if let Err(error) = client.try_subscribe(topic.to_string(), QoS::AtLeastOnce) {
        tracing::error!(
            %topic,
            %error,
            "could not queue the command subscription; the bridge births anyway \
             but can receive no command this session"
        );
    }
}

/// Queues one message. A full queue is a traced drop, never a block: a blocked
/// driver stops draining the inbox, and then NOTHING is published.
fn publish(client: &AsyncClient, message: Outbound) {
    let (qos, retain) = qos_for(message.message);
    if let Err(error) = client.try_publish(message.topic.clone(), qos, retain, message.payload) {
        tracing::warn!(
            topic = %message.topic,
            %error,
            "outbound queue full; message dropped (never silently)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_edge_node_message_is_qos_zero_and_never_retained() {
        // The Sparkplug specification requires QoS 0 / retain false for all
        // edge-node messages; a retained payload would be a value replayed to a
        // new subscriber with no way to judge its age.
        //
        // `MessageType::NCmd` is ABSENT ON PURPOSE — do not "complete" this
        // list. NCMD is inbound: the bridge subscribes to it and never publishes
        // one. Adding it would assert a publish rule about a message we do not
        // send, and the assertion would PASS — a test green for a reason
        // unrelated to the property it names. That is the exact shape of the
        // four Epic 1 tests this project had to throw away.
        //
        // The QoS that does govern NCMD is the SUBSCRIBE QoS, mandated as 1 by
        // `tck-id-message-flow-edge-node-ncmd-subscribe`; it lives in
        // `subscribe_to_commands`, not here.
        for message in [
            MessageType::NBirth,
            MessageType::NData,
            MessageType::NDeath,
            MessageType::DBirth,
            MessageType::DData,
            MessageType::DDeath,
        ] {
            assert_eq!(
                qos_for(message),
                (QoS::AtMostOnce, false),
                "{message:?} must be QoS 0, not retained"
            );
        }
    }

    /// Story 4.6 / AC2 — the broker's answer is read, and the three shapes that
    /// are NOT a clean grant are each distinguishable.
    ///
    /// This is the byte Story 4.4's observer threw away: a refusal is return
    /// code `0x80`, which is neither an error nor a disconnect, so a discarded
    /// answer reads exactly like a topic nobody publishes on. A downgrade to
    /// QoS 0 is just as silent, and it matters because the clause is a MUST on
    /// QoS 1.
    ///
    /// Falsified 2026-07-29: collapsing `granted` to `Granted::AsRequired` for
    /// every input turns three of these four assertions red.
    #[test]
    fn the_brokers_answer_to_the_subscription_is_read_not_assumed() {
        assert_eq!(
            granted(&[SubscribeReasonCode::Success(QoS::AtLeastOnce)]),
            Granted::AsRequired,
            "QoS 1 is what tck-id-message-flow-edge-node-ncmd-subscribe requires"
        );
        assert_eq!(
            granted(&[SubscribeReasonCode::Failure]),
            Granted::Refused,
            "return code 0x80 is a REFUSAL; reporting it as a grant is the \
             false-negative this assertion exists for"
        );
        assert_eq!(
            granted(&[SubscribeReasonCode::Success(QoS::AtMostOnce)]),
            Granted::NotAsRequired(QoS::AtMostOnce),
            "a silent downgrade to QoS 0 leaves the bridge non-conformant and \
             only this byte says so"
        );
        assert_eq!(
            granted(&[]),
            Granted::Empty,
            "a SubAck with no return code is not a grant"
        );
        assert_eq!(
            granted(&[
                SubscribeReasonCode::Success(QoS::AtLeastOnce),
                SubscribeReasonCode::Failure,
            ]),
            Granted::Unattributable(2),
            "this driver issues ONE topic filter per connect, so two return codes \
             mean the answer is not ours alone; reading the first one would report \
             a refused STATE subscription (Story 4.5) as a refused NCMD one"
        );
    }

    /// Captures what a trace macro actually wrote, so a log line can be an
    /// assertion rather than a hope.
    #[derive(Clone, Default)]
    struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Captured {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().expect("not poisoned").clone()).expect("utf-8")
        }
    }

    impl std::io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("not poisoned").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Runs `body` with every trace captured, and returns what was written.
    fn captured(body: impl FnOnce()) -> String {
        let sink = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, body);
        sink.text()
    }

    /// Story 4.6 / AC2 — the SEVERITY and the CONTENT of the answer, not just
    /// its classification.
    ///
    /// Added by the Story 4.6 review, which found that
    /// `the_brokers_answer_to_the_subscription_is_read_not_assumed` proves only
    /// that [`granted`] classifies correctly. Swapping the bodies of two arms of
    /// [`trace_subscription_outcome`] — so a refusal announces itself as a grant
    /// — left the whole suite green. AC2 is about what an operator can see, so
    /// the level and the words are the property under test.
    ///
    /// Falsified 2026-07-29: swapping the `Refused` and `AsRequired` arm bodies
    /// turns the first two assertions red; downgrading the `Refused` arm from
    /// `error!` to `info!` turns its level assertion red on its own.
    #[test]
    fn a_refused_subscription_announces_itself_as_a_refusal_and_at_error() {
        let log = captured(|| {
            trace_subscription_outcome("spBv1.0/G/NCMD/N", Granted::Refused);
        });
        assert!(
            log.contains("ERROR"),
            "a refusal the operator must act on cannot be below ERROR; got: {log}"
        );
        assert!(
            log.contains("REFUSED"),
            "the line must say the subscription was refused; got: {log}"
        );
        assert!(
            log.contains("spBv1.0/G/NCMD/N"),
            "AC2 requires the topic to be named; got: {log}"
        );
    }

    /// Story 4.6 / AC2 — a silent downgrade is reported, at WARN, with the value.
    ///
    /// Falsified 2026-07-29: swapping this arm's body with the `AsRequired` one
    /// turns all three assertions red.
    #[test]
    fn a_downgraded_subscription_names_the_granted_qos_and_at_warn() {
        let log = captured(|| {
            trace_subscription_outcome("spBv1.0/G/NCMD/N", Granted::NotAsRequired(QoS::AtMostOnce));
        });
        assert!(
            log.contains("WARN"),
            "AC2 puts a downgrade at WARN; got: {log}"
        );
        assert!(
            log.contains("AtMostOnce"),
            "AC2 requires the GRANTED value to be named, not just that it was \
             wrong; got: {log}"
        );
        assert!(
            !log.contains("granted the command subscription at QoS 1"),
            "a downgrade must not read as a clean grant; got: {log}"
        );
    }

    /// Story 4.6 / AC2 — a clean grant is INFO and says so.
    ///
    /// Present so that the three bad shapes cannot be made to pass by making
    /// EVERY outcome shout: if the happy path were also ERROR, an operator
    /// grepping for trouble would find it every session.
    ///
    /// Falsified 2026-07-29: raising this arm to `error!` turns the level
    /// assertion red.
    #[test]
    fn a_clean_grant_is_reported_at_info_and_not_as_a_problem() {
        let log = captured(|| {
            trace_subscription_outcome("spBv1.0/G/NCMD/N", Granted::AsRequired);
        });
        assert!(log.contains("INFO"), "a grant is not a problem; got: {log}");
        assert!(
            !log.contains("ERROR") && !log.contains("WARN"),
            "the happy path must not be indistinguishable from a failure; got: {log}"
        );
    }

    /// Story 4.6 / AC3 — the three ignore shapes are DISTINGUISHABLE in the log.
    ///
    /// Added by the Story 4.6 review. The chaos test asserted the metric NAME
    /// (`Node Control/Rebirth`) rather than the ignore trace, so a future handler
    /// that ACTED on the command while logging its name would have kept that
    /// assertion green.
    ///
    /// Falsified 2026-07-29: giving all three arms of `trace_command_outcome` the
    /// same message turns the distinctness assertions red; dropping the `?names`
    /// field turns the name assertion red.
    #[test]
    fn the_three_ignore_shapes_do_not_read_alike() {
        let unrecognised = captured(|| {
            trace_command_outcome(
                "t",
                Inbound::Unrecognised(vec!["Node Control/Rebirth".to_string()]),
            );
        });
        let undecodable = captured(|| {
            trace_command_outcome("t", Inbound::Undecodable("bad varint".to_string()));
        });
        let empty = captured(|| trace_command_outcome("t", Inbound::NoMetrics));

        for log in [&unrecognised, &undecodable, &empty] {
            assert!(
                log.contains("ignored"),
                "every one of these paths must say the command was ignored; got: {log}"
            );
        }
        assert!(
            unrecognised.contains("Node Control/Rebirth"),
            "AC3 requires the metric names to be traced; got: {unrecognised}"
        );
        assert!(
            undecodable.contains("WARN") && undecodable.contains("not a Sparkplug payload"),
            "an undecodable payload is a WARN and says why; got: {undecodable}"
        );
        assert!(
            empty.contains("no metric"),
            "a decoded-but-empty payload is not silently dropped; got: {empty}"
        );
        // The three must not collapse into one another.
        assert_ne!(
            unrecognised.split_whitespace().collect::<Vec<_>>(),
            undecodable.split_whitespace().collect::<Vec<_>>()
        );
        assert_ne!(
            unrecognised.split_whitespace().collect::<Vec<_>>(),
            empty.split_whitespace().collect::<Vec<_>>()
        );
    }

    fn command_payload(names: &[&str]) -> Vec<u8> {
        let metrics = names
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

    /// Story 4.6 / AC3 — a payload that does not decode is an ORDINARY input.
    ///
    /// It arrives from the network, so anyone can send one; `.expect()` here
    /// would let a stranger stop the bridge by publishing eleven bytes.
    ///
    /// Falsified 2026-07-29: replacing the `Err` arm of `classify` with
    /// `.expect("a Sparkplug payload")` turns this red — the test panics instead
    /// of returning a verdict, which is precisely the production behaviour it
    /// forbids.
    #[test]
    fn a_command_that_does_not_decode_is_classified_never_unwrapped() {
        // A varint whose continuation bit never clears: not a Sparkplug payload
        // by any reading.
        let garbage = [0xffu8; 11];
        assert!(
            matches!(classify(&garbage), Inbound::Undecodable(_)),
            "malformed bytes must produce a verdict, not a panic"
        );
    }

    /// Story 4.6 / AC3 — every command is unrecognised in this story, and the
    /// trace names what arrived.
    ///
    /// `Node Control/Rebirth` is used as the sample deliberately: it is the one
    /// command a live MQTT Engine actually sends, and Story 4.7 — not this one —
    /// is where it acquires meaning. Seeing it here as `Unrecognised` is the
    /// correct answer today.
    ///
    /// Falsified 2026-07-29: making `classify` return `Inbound::NoMetrics` for
    /// every decoded payload turns both assertions red.
    #[test]
    fn a_recognisable_looking_command_is_still_unrecognised_in_this_story() {
        let bytes = command_payload(&["Node Control/Rebirth", "Node Control/Next Server"]);
        assert_eq!(
            classify(&bytes),
            Inbound::Unrecognised(vec![
                "Node Control/Rebirth".to_string(),
                "Node Control/Next Server".to_string(),
            ])
        );
    }

    /// Story 4.6 / AC3 — a metric addressed by ALIAS still identifies itself.
    ///
    /// Added by the Story 4.6 review. Sparkplug lets a host address a metric by
    /// alias instead of by name; this bridge publishes `alias: None` on
    /// everything it sends, but that governs what it emits, not what a host may
    /// send it. Every existing `command_payload` sets a name, so the fallback
    /// branch had no test at all.
    ///
    /// It matters beyond tidiness: an alias-addressed `Node Control/Rebirth`
    /// would have traced `names=["<unnamed>"]` — a name list whose names were
    /// literally lost, which is the confusion the `NoMetrics` arm exists to
    /// prevent — and Story 4.7's name-matching handler would silently never
    /// fire, with a log line that looks like ordinary operation.
    ///
    /// Falsified 2026-07-29: restoring the `unwrap_or_else(|| "<unnamed>")`
    /// fallback turns the first assertion red.
    #[test]
    fn a_command_addressed_by_alias_is_not_reported_as_nameless() {
        let by_alias = sparkplug_b::encode(&sparkplug_b::protobuf::Payload {
            timestamp: Some(1_700_000_000_000),
            metrics: vec![
                sparkplug_b::protobuf::payload::Metric {
                    name: None,
                    alias: Some(7),
                    ..Default::default()
                },
                sparkplug_b::protobuf::payload::Metric {
                    name: None,
                    alias: None,
                    ..Default::default()
                },
            ],
            seq: None,
            uuid: None,
            body: None,
        });
        assert_eq!(
            classify(&by_alias),
            Inbound::Unrecognised(vec![
                "<alias 7>".to_string(),
                "<neither name nor alias>".to_string(),
            ]),
            "an alias is an identity; collapsing it to <unnamed> throws away the \
             only thing that says which command arrived"
        );
    }

    /// Story 4.6 / AC3 — a well-formed payload carrying nothing is traced too.
    ///
    /// Without its own arm it would fall into the "unrecognised" branch with an
    /// empty name list, which reads in a log as a command whose names were lost
    /// rather than a command that carried none.
    ///
    /// Falsified 2026-07-29: deleting the `metrics.is_empty()` arm makes this
    /// return `Unrecognised([])` and the test goes red.
    #[test]
    fn a_command_carrying_no_metric_is_not_silently_dropped() {
        assert_eq!(classify(&command_payload(&[])), Inbound::NoMetrics);
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("smartme_bdseq_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    #[test]
    fn bd_seq_survives_a_round_trip_and_a_missing_file() {
        let path = temp("roundtrip.toml");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            load_bd_seq(&path),
            BdSeq::before_first(),
            "a missing file means we start from the sentinel, not a guess"
        );
        store_bd_seq(&path, BdSeq::new(42)).expect("persist");
        assert_eq!(load_bd_seq(&path), BdSeq::new(42));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_bd_seq_file_falls_back_to_the_sentinel() {
        let path = temp("corrupt.toml");
        std::fs::write(&path, "this is not toml {{{").expect("write");
        assert_eq!(load_bd_seq(&path), BdSeq::before_first());
        let _ = std::fs::remove_file(&path);
    }
}
