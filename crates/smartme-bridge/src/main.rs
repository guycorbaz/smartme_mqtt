//! Thin shell over `lib::run()` — the wiring lives in the library so the
//! integration and chaos tests can drive the same code the binary runs.
//!
//! Configuration is read from the environment for the walking skeleton; Epic 3
//! replaces this with the validated config file. Secrets stay env-only and are
//! never logged (NFR12).

use std::path::PathBuf;
use std::time::Duration;

use smart_me_client::{Credentials, SmartMeClient};
use smartme_bridge::app::{BridgeConfig, PollConfig};
use smartme_bridge::core::state_machine::Policy;
use smartme_bridge::domain::{MeterId, Serial};

fn env(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("missing environment variable {key}"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let credentials = Credentials::ClientCredentials {
        client_id: env("SMARTME_CLIENT_ID")?,
        client_secret: env("SMARTME_CLIENT_SECRET")?,
    };
    let config = BridgeConfig {
        api_base: std::env::var("SMARTME_API_BASE")
            .unwrap_or_else(|_| SmartMeClient::DEFAULT_BASE.to_string()),
        credentials,
        http_timeout: Duration::from_secs(10),
        meter: MeterId::new(env("SMARTME_METER_ID")?),
        device_id: env("SMARTME_DEVICE_ID")?,
        serial: Serial::new(env("SMARTME_SERIAL")?),
        group_id: std::env::var("SMARTME_GROUP_ID").unwrap_or_else(|_| "Site".to_string()),
        node_id: std::env::var("SMARTME_NODE_ID").unwrap_or_else(|_| "Bridge".to_string()),
        broker_host: env("SMARTME_BROKER_HOST")?,
        // A typo'd port must not silently connect somewhere else.
        broker_port: match std::env::var("SMARTME_BROKER_PORT") {
            Ok(raw) => raw
                .trim()
                .parse()
                .map_err(|_| format!("SMARTME_BROKER_PORT is not a port number: {raw:?}"))?,
            Err(_) => 1883,
        },
        bd_seq_path: PathBuf::from(
            std::env::var("SMARTME_STATE_DIR").unwrap_or_else(|_| "/data".to_string()),
        )
        .join("bdseq.toml"),
        poll: PollConfig {
            interval: Duration::from_secs(30),
            fetch_timeout: Duration::from_secs(10),
        },
        policy: Policy { max_age_ms: 90_000 },
    };

    smartme_bridge::run(config)
}
