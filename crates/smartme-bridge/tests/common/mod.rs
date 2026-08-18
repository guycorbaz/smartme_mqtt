//! Shared harness for the chaos tier: a real broker in a container and an
//! INDEPENDENT subscriber.
//!
//! Independent is the whole point. These tests assert what a third party
//! actually receives, never what the bridge believes it sent — a bridge that
//! lied to a SCADA host would lie to its own logs just as convincingly.
//!
//! Each chaos binary compiles this module separately and uses a different part
//! of it, so unused-code warnings here are an artefact of that, not a smell.
#![allow(dead_code)]

use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::sync::mpsc;

/// One message as seen from outside the bridge.
#[derive(Debug, Clone)]
pub struct Seen {
    /// Topic it arrived on.
    pub topic: String,
    /// Decoded Sparkplug payload.
    pub payload: sparkplug_b::protobuf::Payload,
}

impl Seen {
    /// The quality property of a named metric, if present.
    pub fn quality_of(&self, metric: &str) -> Option<u32> {
        let m = self
            .payload
            .metrics
            .iter()
            .find(|m| m.name.as_deref() == Some(metric))?;
        let props = m.properties.as_ref()?;
        let idx = props
            .keys
            .iter()
            .position(|k| k == sparkplug_b::Quality::PROPERTY_KEY)?;
        match props.values.get(idx)?.value {
            Some(sparkplug_b::protobuf::payload::property_value::Value::IntValue(v)) => Some(v),
            _ => None,
        }
    }

    /// Whether a named metric carries an actual value (rather than being null).
    pub fn has_value(&self, metric: &str) -> bool {
        self.payload
            .metrics
            .iter()
            .find(|m| m.name.as_deref() == Some(metric))
            .and_then(|m| m.value.as_ref())
            .is_some()
    }

    /// The `bdSeq` carried by a BIRTH or DEATH.
    pub fn bd_seq(&self) -> Option<i64> {
        let m = self
            .payload
            .metrics
            .iter()
            .find(|m| m.name.as_deref() == Some(sparkplug_b::BD_SEQ_METRIC))?;
        match m.value {
            Some(sparkplug_b::protobuf::payload::metric::Value::LongValue(v)) => Some(v as i64),
            _ => None,
        }
    }
}

/// Starts a broker container and returns it with its mapped host port.
pub async fn start_broker() -> (ContainerAsync<GenericImage>, u16) {
    let container = GenericImage::new("eclipse-mosquitto", "2")
        .with_wait_for(WaitFor::message_on_stderr("mosquitto version 2"))
        .with_cmd(vec!["mosquitto", "-c", "/mosquitto-no-auth.conf"])
        .start()
        .await
        .expect("broker container starts");
    let port = container
        .get_host_port_ipv4(1883.tcp())
        .await
        .expect("broker port is mapped");
    (container, port)
}

/// As [`start_broker`], but on a host port that **survives a restart of the
/// container** — the one property story 4.13 cannot do without.
///
/// [`start_broker`] lets Docker pick the host port. Stop that container and
/// start it again and the mapping is redrawn: the broker comes back on a
/// *different* host port, while the bridge under test still holds the old one.
/// It then reconnects forever against nothing, and the test does not fail — it
/// **hangs to its timeout**, which is the shape this repository keeps having to
/// tell apart from a real defect. `with_mapped_port` pins the host side into the
/// container's own configuration, so `stop` + `start` reopens the same door.
///
/// **The port is chosen, not hardcoded.** ADR 0037 exists because a fixed 8080
/// blocked the full gate twice on this machine — *"d'autres développements sont
/// en cours dans d'autres fenêtres"* is the normal state here, not an accident.
/// [`an_unused_host_port`] asks the kernel for one instead.
///
/// Kept BESIDE [`start_broker`] rather than replacing it: every other chaos test
/// wants the ephemeral mapping, which cannot collide with anything, and a fixed
/// mapping — however well chosen — carries a race no ephemeral one has.
pub async fn start_broker_on_fixed_port() -> (ContainerAsync<GenericImage>, u16) {
    let port = an_unused_host_port();
    let container = GenericImage::new("eclipse-mosquitto", "2")
        .with_wait_for(WaitFor::message_on_stderr("mosquitto version 2"))
        .with_cmd(vec!["mosquitto", "-c", "/mosquitto-no-auth.conf"])
        .with_mapped_port(port, 1883.tcp())
        .start()
        .await
        .expect("broker container starts on the requested host port");
    (container, port)
}

/// A host port nothing holds *at this instant*, obtained by binding port 0 and
/// letting the kernel answer.
///
/// There is a window between the release here and Docker's bind, and it cannot
/// be closed — a mapping must be named before the container exists. It is
/// narrow, and the alternative is a constant, which ADR 0037 measured the cost
/// of twice. Losing the race produces a container that refuses to start with
/// `address already in use`, which is loud; holding a constant produces a
/// container that refuses to start for the same reason *every* time another
/// project is running, which is what actually happened.
fn an_unused_host_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("the kernel has a port");
    listener
        .local_addr()
        .expect("a bound listener has an address")
        .port()
}

/// Blocks until the broker on `port` completes an MQTT **CONNACK**, or the
/// deadline passes.
///
/// **`ContainerAsync::start` does not re-apply the image's `WaitFor`** — it
/// issues the Docker start and returns. A test that published straight after it
/// would be racing mosquitto's own boot, and losing that race looks exactly like
/// a bridge that failed to reconnect.
///
/// # It waits for CONNACK, and a TCP handshake was NOT enough
///
/// The first version of this returned as soon as `TcpStream::connect` succeeded,
/// and that made this test fail roughly once in six with `no NBIRTH arrived
/// within 60 s`. **`docker-proxy` binds the host port before mosquitto is
/// accepting**, so the TCP handshake is answered by Docker on the broker's
/// behalf while the broker is still starting. In that window the test declares
/// the outage over, and any reconnect attempt the bridge makes inside it spends
/// `rumqttc`'s five-second connection timeout and then **doubles the backoff** —
/// straight to the 30 s ceiling, plus up to 50 % jitter. The bridge was behaving
/// correctly the whole time; the harness had simply lied about when the broker
/// came back.
///
/// Returns whether the broker answered, so a caller can say so in its own words
/// rather than inheriting a panic from here.
pub async fn wait_for_broker(port: u16, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut attempt = 0u32;
    while tokio::time::Instant::now() < deadline {
        attempt += 1;
        // A fresh client id per attempt: a half-started broker may accept and
        // then drop, and reusing one id would have each attempt evict the
        // session the previous one left behind.
        let mut options = MqttOptions::new(
            format!("broker-readiness-probe-{attempt}"),
            "127.0.0.1",
            port,
        );
        options.set_keep_alive(Duration::from_secs(5));
        let (_client, mut eventloop) = AsyncClient::new(options, 4);
        let connacked = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => return true,
                    Ok(_) => {}
                    Err(_) => return false,
                }
            }
        })
        .await
        .unwrap_or(false);
        if connacked {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// As [`start_broker`], but with `mosquitto -v`, so the broker records every
/// control packet it receives.
///
/// This exists because one MQTT client cannot observe another client's
/// SUBSCRIBE: subscriptions are between a client and the broker, and nothing is
/// published when one is made. The broker's own log is therefore the only
/// external oracle for the ordering
/// `tck-id-message-flow-edge-node-ncmd-subscribe` requires — *"prior to sending
/// an NBIRTH message"*. Verbose mosquitto writes, in receipt order:
///
/// ```text
/// Received SUBSCRIBE from <client_id>
///     spBv1.0/<group>/NCMD/<node> (QoS 1)
/// Received PUBLISH from <client_id> (d0, q0, r0, m0, 'spBv1.0/<group>/NBIRTH/<node>', ...)
/// ```
///
/// Kept separate from [`start_broker`] rather than switched on globally: the
/// other chaos tests do not read the broker's log, and a container producing a
/// line per packet would only make their failures harder to read.
pub async fn start_verbose_broker() -> (ContainerAsync<GenericImage>, u16) {
    let container = GenericImage::new("eclipse-mosquitto", "2")
        .with_wait_for(WaitFor::message_on_stderr("mosquitto version 2"))
        .with_cmd(vec!["mosquitto", "-c", "/mosquitto-no-auth.conf", "-v"])
        .start()
        .await
        .expect("broker container starts");
    let port = container
        .get_host_port_ipv4(1883.tcp())
        .await
        .expect("broker port is mapped");
    (container, port)
}

/// Subscribes to everything under the Sparkplug namespace and streams what
/// arrives. This client has no relationship with the bridge beyond the broker.
pub async fn independent_subscriber(port: u16) -> mpsc::Receiver<Seen> {
    named_subscriber(port, "independent-observer").await
}

/// As [`independent_subscriber`], but with an explicit client id.
///
/// A test that wants a SECOND observer must name it differently: a broker
/// evicts the older session when a client id reconnects, so two observers
/// sharing a name would silently unplug each other.
pub async fn named_subscriber(port: u16, client_id: &str) -> mpsc::Receiver<Seen> {
    named_subscriber_on("127.0.0.1", port, client_id).await
}

/// As [`named_subscriber`], but against a broker that is not on loopback.
///
/// Used by the manual confirmation run against a real broker; the containerised
/// tests never need it.
pub async fn named_subscriber_on(host: &str, port: u16, client_id: &str) -> mpsc::Receiver<Seen> {
    let mut options = MqttOptions::new(client_id, host, port);
    options.set_keep_alive(Duration::from_secs(5));
    let (client, mut eventloop) = AsyncClient::new(options, 64);
    let (tx, rx) = mpsc::channel(256);
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let mut ready_tx = Some(ready_tx);
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    client
                        .subscribe("spBv1.0/#", QoS::AtMostOnce)
                        .await
                        .expect("subscribe");
                }
                Ok(Event::Incoming(Packet::SubAck(_))) => {
                    if let Some(tx) = ready_tx.take() {
                        let _ = tx.send(());
                    }
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    if let Ok(payload) = sparkplug_b::decode(&p.payload) {
                        let seen = Seen {
                            topic: p.topic.clone(),
                            payload,
                        };
                        if tx.send(seen).await.is_err() {
                            return;
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    });
    // Subscribe BEFORE the bridge is allowed to publish: a test that races its
    // own observer proves nothing.
    ready_rx.await.expect("observer subscribed");
    rx
}

/// Discards everything already buffered, so a later `wait_for` cannot be
/// satisfied by a message that arrived before the thing it is waiting on.
///
/// Added by the Story 4.7 code review. `wait_for` accepts the FIRST match it
/// finds, which makes it structurally unable to distinguish "the event I provoked"
/// from "an event of the same shape that was already in the queue" — and
/// `chaos_ncmd_subscription` forces a broker eviction, and therefore a reconnect
/// BIRTH, shortly before asserting that a command produced a BIRTH. Draining is
/// what makes that assertion about the command.
pub async fn drain(rx: &mut mpsc::Receiver<Seen>) {
    while rx.try_recv().is_ok() {}
}

/// Waits for the first message matching `predicate`, or gives up.
pub async fn wait_for(
    rx: &mut mpsc::Receiver<Seen>,
    timeout: Duration,
    predicate: impl Fn(&Seen) -> bool,
) -> Option<Seen> {
    tokio::time::timeout(timeout, async {
        while let Some(seen) = rx.recv().await {
            if predicate(&seen) {
                return Some(seen);
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

/// Write the `config.toml` a spawned bridge will read (ADR 0023).
///
/// **Added 2026-08-04, and the two tests that spawn the real binary would
/// otherwise have gone quiet rather than red.** They used to configure the
/// bridge through `SMARTME_GROUP_ID` and its nine companions; those variables
/// were withdrawn when the file became the configuration. A binary spawned with
/// them and no file does not fail — it comes up UNCONFIGURED, publishes nothing,
/// and waits, so every assertion about what a subscriber receives would have
/// timed out against a bridge that was working exactly as designed.
///
/// The Sparkplug identity and the meter are parameters because each chaos test
/// needs its own group, so that two runs against the shared broker cannot see
/// each other's births.
pub fn write_config(
    dir: &std::path::Path,
    group: &str,
    node: &str,
    serial: &str,
    broker_host: &str,
    broker_port: u16,
) {
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "schema_version = {}\n\
             group_id = \"{group}\"\n\
             node_id = \"{node}\"\n\
             broker_host = \"{broker_host}\"\n\
             broker_port = {broker_port}\n\
             publish_period_secs = 30\n\
             # CONFIRMED, because these tests are about what reaches the wire\n\
             # and not about the confirmation gate (Story 5.3). Without this the\n\
             # spawned binary comes up and publishes NOTHING — and the tests do\n\
             # not fail, they TIME OUT against a bridge behaving exactly as\n\
             # designed. That is the fifth time in one day that a change to how\n\
             # the bridge starts went quiet rather than red here.\n\
             mapping_confirmed = true\n\
             # TEST-NET-1 (RFC 5737): unroutable, so the cloud stays silent for\n\
             # the whole run. The Sparkplug session does not depend on having a\n\
             # reading, so the node still births.\n\
             api_base = \"https://192.0.2.1\"\n\
             \n\
             [[meters]]\n\
             meter_id = \"garage\"\n\
             device_id = \"a1a1a1a1-b2b2-c3c3-d4d4-000000000001\"\n\
             serial = \"{serial}\"\n\
             enabled = true\n",
            smartme_bridge::app::store::SCHEMA_VERSION
        ),
    )
    .expect("write the bridge's configuration");
}
