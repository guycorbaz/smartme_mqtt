//! The supervisor (Story 1.13): the two tasks are born whole, and death is
//! guaranteed on shutdown.
//!
//! SIGTERM-NO-LIE: a clean stop must never leave the SCADA host showing a stale
//! value as live. Two mechanisms cover it — an explicit DEATH published before
//! exit, and the broker's last will if we die before we can publish it. Either
//! way the consumer learns the node is gone.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::adapters::SmartMeCloudSource;
use crate::app::mqtt_driver::{self, MqttConfig};
use crate::app::poll_publish::{self, LastLoopTick, PollConfig};
use crate::core::clock::{Clock, SystemClock};
use crate::core::state_machine::Policy;
use crate::domain::{MeterId, Serial};
use smart_me_client::{Credentials, SmartMeClient};

/// Everything the bridge needs to run. Assembled by the caller (Epic 3 reads it
/// from a config file); nothing here reaches for an environment variable on its
/// own, so a test can build one by hand.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// smart-me API base URL.
    pub api_base: String,
    /// smart-me credentials.
    pub credentials: Credentials,
    /// Per-request HTTP timeout.
    pub http_timeout: Duration,
    /// The logical meter served by this bridge (walking skeleton: exactly one).
    pub meter: MeterId,
    /// smart-me device id backing that meter.
    pub device_id: String,
    /// The device serial, used as the Sparkplug device identifier.
    pub serial: Serial,
    /// Sparkplug group identifier.
    pub group_id: String,
    /// Sparkplug edge node identifier.
    pub node_id: String,
    /// Broker host.
    pub broker_host: String,
    /// Broker port.
    pub broker_port: u16,
    /// Where the session number is persisted.
    pub bd_seq_path: PathBuf,
    /// Loop pacing and fetch deadline.
    pub poll: PollConfig,
    /// Staleness policy.
    pub policy: Policy,
}

/// Why the bridge could not start. Refusing to start beats starting wrong: a
/// half-configured bridge publishes confident nonsense.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// The smart-me client refused its configuration (non-TLS endpoint, ...).
    #[error("smart-me client: {0}")]
    Client(#[from] smart_me_client::SmartMeError),
    /// The Sparkplug identifiers are not usable as topic levels.
    #[error("sparkplug identifiers: {0}")]
    Topic(#[from] sparkplug_b::TopicError),
}

/// Builds and runs the bridge until `shutdown` resolves.
///
/// Both tasks are spawned together and the channel between them is created
/// first: neither can exist meaningfully without the other, so there is no
/// window where one is running alone.
pub async fn run(
    config: BridgeConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), StartupError> {
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(config.group_id.clone(), config.node_id.clone())?;
    // Validate the device identifier HERE: a serial that cannot be a topic level
    // would otherwise leave the node connected, unborn and publishing nothing,
    // forever. Refusing to start beats starting wrong.
    node.device_topic(sparkplug_b::MessageType::DBirth, config.serial.as_str())?;

    let client = SmartMeClient::new(
        config.api_base.clone(),
        config.credentials.clone(),
        config.http_timeout,
    )?;
    let source = SmartMeCloudSource::new(
        client,
        Arc::clone(&clock),
        config.meter.clone(),
        config.device_id.clone(),
    );

    // The channel first: the seam exists before either end does.
    let (tx, rx) = mpsc::channel(64);
    let heartbeat = LastLoopTick::new();
    let (death_tx, death_rx) = oneshot::channel();

    let mqtt = tokio::spawn(mqtt_driver::run(
        MqttConfig {
            client_id: config.node_id.clone(),
            host: config.broker_host.clone(),
            port: config.broker_port,
            keep_alive: Duration::from_secs(30),
            bd_seq_path: config.bd_seq_path.clone(),
            capacity: 64,
            death_flush: Duration::from_secs(2),
        },
        node,
        vec![config.serial.clone()],
        Arc::clone(&clock),
        rx,
        death_rx,
    ));

    let poll = tokio::spawn(poll_publish::run(
        config.meter.clone(),
        source,
        Arc::clone(&clock),
        config.policy,
        config.poll,
        heartbeat,
        tx,
    ));

    shutdown.await;
    tracing::info!("shutdown signalled");

    // Tell the driver to publish the death, THEN stop the poll task: the order
    // matters only in that the death must not be blocked behind a pending
    // reading.
    let _ = death_tx.send(());
    poll.abort();
    // Wait for the driver to finish publishing the certificate. If it panicked,
    // the connection drops and the broker's will fires — the second mechanism.
    if let Err(error) = mqtt.await {
        tracing::error!(%error, "mqtt driver did not stop cleanly; the will covers us");
    }
    Ok(())
}

/// Resolves when the process is asked to stop (SIGTERM in a container, Ctrl-C
/// interactively).
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(error) => {
                tracing::error!(%error, "cannot listen for SIGTERM; Ctrl-C only");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BridgeConfig {
        BridgeConfig {
            api_base: "https://api.smart-me.com".to_string(),
            credentials: Credentials::Basic {
                user: "u".to_string(),
                password: "p".to_string(),
            },
            http_timeout: Duration::from_secs(10),
            meter: MeterId::new("garage"),
            device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000001".to_string(),
            serial: Serial::new("30000001"),
            group_id: "Site".to_string(),
            node_id: "Bridge".to_string(),
            broker_host: "127.0.0.1".to_string(),
            broker_port: 1883,
            bd_seq_path: std::env::temp_dir().join("smartme_supervisor_test_bdseq.toml"),
            poll: PollConfig {
                interval: Duration::from_secs(5),
                fetch_timeout: Duration::from_secs(2),
            },
            policy: Policy { max_age_ms: 90_000 },
        }
    }

    #[tokio::test]
    async fn a_non_tls_api_base_refuses_to_start() {
        let mut c = config();
        c.api_base = "http://api.smart-me.com".to_string();
        let err = run(c, async {}).await.expect_err("must refuse");
        assert!(matches!(err, StartupError::Client(_)));
    }

    #[tokio::test]
    async fn an_identifier_that_cannot_be_a_topic_level_refuses_to_start() {
        let mut c = config();
        c.node_id = "Bridge/One".to_string();
        let err = run(c, async {}).await.expect_err("must refuse");
        assert!(matches!(err, StartupError::Topic(_)));
    }

    #[tokio::test]
    async fn an_immediate_shutdown_still_completes_the_lifecycle() {
        // No broker is listening: the driver cannot connect, but the supervisor
        // must still shut down rather than hang — a bridge that will not stop is
        // a bridge that cannot be restarted honestly.
        let c = config();
        let result = tokio::time::timeout(Duration::from_secs(10), run(c, async {})).await;
        assert!(result.is_ok(), "shutdown must not hang without a broker");
        assert!(result.expect("completed").is_ok());
    }
}
