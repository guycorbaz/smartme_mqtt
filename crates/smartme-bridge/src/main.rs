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

/// Default number of rotated log files kept when `SMARTME_LOG_KEEP` is unset.
/// Rotation is daily, so this is a retention window in days.
const DEFAULT_LOG_KEEP: usize = 7;

/// What must outlive `run()` for file logging to keep working, plus the means to
/// report at the end how much of it was lost.
struct FileLog {
    /// Dropping this flushes and stops the writer thread — hence `_`.
    _worker: tracing_appender::non_blocking::WorkerGuard,
    dropped: tracing_appender::non_blocking::ErrorCounter,
    directory: PathBuf,
}

/// Builds the file-logging layer, or `None` when file logging is not configured
/// or not possible.
///
/// **Opt-in, deliberately.** File logging exists only when `SMARTME_LOG_DIR` is
/// set. With it unset the binary behaves exactly as it did before this was
/// added — which is what the two chaos tests that spawn the real binary and read
/// its output depend on. A default path would have made a comfort feature able
/// to break the tests that guard the product's honesty.
///
/// **Never fatal.** A bridge that stops publishing because it cannot write a log
/// file has turned a diagnostic aid into an outage. Every failure here degrades
/// to console-only and says so on stderr — loudly, because the failure this
/// whole change was requested to diagnose (`bdSeq`, `Permission denied`) was one
/// nobody saw until they went looking for a file that was never there.
fn file_log_layer<S>() -> (
    Option<Box<dyn tracing_subscriber::Layer<S> + Send + Sync>>,
    Option<FileLog>,
)
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    use tracing_subscriber::Layer as _;

    let Ok(directory) = std::env::var("SMARTME_LOG_DIR") else {
        return (None, None);
    };
    let directory = PathBuf::from(directory);

    let keep = match std::env::var("SMARTME_LOG_KEEP") {
        Ok(raw) => match raw.trim().parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                eprintln!(
                    "SMARTME_LOG_KEEP is not a positive number ({raw:?}); \
                     keeping {DEFAULT_LOG_KEEP} files"
                );
                DEFAULT_LOG_KEEP
            }
        },
        Err(_) => DEFAULT_LOG_KEEP,
    };

    if let Err(error) = std::fs::create_dir_all(&directory) {
        eprintln!(
            "file logging DISABLED: cannot use {}: {error}. \
             Logs go to the console only. On a container deployment this is \
             usually the bind-mount's ownership: the directory must belong to \
             the uid the container runs as.",
            directory.display()
        );
        return (None, None);
    }

    let appender = match tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("smartme_mqtt")
        .filename_suffix("log")
        .max_log_files(keep)
        .build(&directory)
    {
        Ok(appender) => appender,
        Err(error) => {
            eprintln!(
                "file logging DISABLED: cannot open a log file in {}: {error}. \
                 Logs go to the console only.",
                directory.display()
            );
            return (None, None);
        }
    };

    // Lossy on purpose (chosen 2026-08-01). A saturated buffer drops lines
    // rather than blocking, so a slow disk can never stall the bridge — the
    // project's standing rule is "a traced drop, never a block".
    //
    // What makes that acceptable is that ONLY THE FILE can lose lines: the
    // console layer writes straight to stdout and is not buffered by this
    // writer, so `docker compose logs` remains complete even when the file is
    // not. The count is reported at shutdown, so the loss is never silent.
    let (writer, worker) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .lossy(true)
        .finish(appender);
    let dropped = writer.error_counter();

    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        // No ANSI in a file: escape sequences make `grep` and any log viewer
        // read the level as part of the message.
        .with_ansi(false)
        .boxed();

    (
        Some(layer),
        Some(FileLog {
            _worker: worker,
            dropped,
            directory,
        }),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // INFO by default, and NOT `fmt::init()`.
    //
    // `tracing_subscriber::fmt::init()` resolves to `EnvFilter::from_default_env()`,
    // whose default directive is `LevelFilter::ERROR`. With no `RUST_LOG` in the
    // environment that drops every WARN and INFO — which is most of what this
    // bridge exists to say: the ignored-NCMD traces, the QoS-downgrade warning
    // on a subscription the specification requires at QoS 1, and every traced
    // drop. The operator would see an empty log and read it as "nothing to
    // report" rather than "nothing is being reported".
    //
    // Found by the Story 4.6 review: AC2 and AC3 are both written in terms of
    // what an operator can see in the log, and both were dark by default.
    // `RUST_LOG` still overrides this whenever it is set.
    //
    // The same filter governs both sinks, so the file never contains something
    // the console omitted — one log, two destinations.
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let (file_layer, file_log) = file_log_layer();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(file_layer)
        .init();

    if let Some(log) = &file_log {
        tracing::info!(directory = %log.directory.display(), "logging to file");
    }

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
        // NO DEFAULT, and this is the same rule as the port below.
        //
        // These two identifiers ARE the topic namespace. A Sparkplug host
        // persists what it discovers: the group becomes a folder in its tag
        // tree that outlives the process and has to be deleted by hand, and
        // deleting MQTT Engine tags also discards their alarm and history
        // configuration. Publishing into the wrong namespace is therefore not
        // recoverable by restarting with better settings.
        //
        // They defaulted to `Site` / `Bridge` until 2026-07-31. The asymmetry
        // that exposed it: the Tier-3 contract test REFUSES to publish into a
        // group called `Site`, precisely because that is the production
        // namespace — while the product itself published there whenever the
        // variable was unset. The guard was on the disposable artifact and
        // absent from the real one. Found while standing up a probe against
        // the live broker, where the default had to be overridden by hand to
        // avoid exactly this.
        group_id: env("SMARTME_GROUP_ID")?,
        node_id: env("SMARTME_NODE_ID")?,
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

    let outcome = smartme_bridge::run(config);

    // Report the loss before the guard is dropped. A dropped line is a line the
    // FILE never received; the console has them all. Saying nothing here would
    // leave a truncated file looking like a quiet period.
    if let Some(log) = &file_log {
        let dropped = log.dropped.dropped_lines();
        if dropped > 0 {
            eprintln!(
                "{dropped} log line(s) never reached {} — the write buffer \
                 saturated. The console output above is complete; the file is not.",
                log.directory.display()
            );
        }
    }
    drop(file_log);

    outcome
}
