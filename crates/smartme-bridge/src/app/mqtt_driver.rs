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
//! 5. publish the BIRTH.
//!
//! Get this wrong and the broker holds no will while the node is alive — the
//! node dies and nobody is told, which is the silent lie this project exists to
//! prevent.
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

use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    /// The broker accepted the connection: it is time to birth.
    Connected,
    /// The connection dropped; the will covers us until we reconnect.
    Lost,
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
    let pump = tokio::spawn(pump_transport(eventloop, transport_tx));

    loop {
        tokio::select! {
            event = transport_rx.recv() => {
                match event {
                    Some(Transport::Connected) => {
                        // 5. Connected: publish the BIRTH.
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
                    Some(Transport::Lost) => {
                        tracing::warn!("transport lost; the will covers us until we reconnect");
                    }
                    None => {
                        tracing::error!("transport task ended; stopping");
                        break;
                    }
                }
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
async fn pump_transport(mut eventloop: EventLoop, events: mpsc::Sender<Transport>) {
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                if events.send(Transport::Connected).await.is_err() {
                    return;
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
