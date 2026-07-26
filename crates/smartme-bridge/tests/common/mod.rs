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
