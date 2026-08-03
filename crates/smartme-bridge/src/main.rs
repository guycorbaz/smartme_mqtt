//! Thin shell over `lib::run()` — the wiring lives in the library so the
//! integration and chaos tests can drive the same code the binary runs.
//!
//! Configuration is read from the environment into `app::config::RawConfig` and
//! validated by `app::config::validate` — this file applies no rule of its own.
//! The environment is the BOOTSTRAP path (FR23); FR46 adds the web UI as a
//! second source feeding the same validation. Secrets are never logged, and
//! never reach a fault message (NFR12, ADR 0019).

use std::path::PathBuf;

use smartme_bridge::app::config::{RawConfig, RawMeter};

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

    // THE FIRST LINE, and the position is the whole point.
    //
    // It is emitted before anything that can fail — before the configuration is
    // read, and therefore before the identity guard can abort the process. A
    // banner printed after `env("SMARTME_GROUP_ID")?` would be absent from the
    // one log an operator most needs it in: the crash-looping container. Today
    // such a container's entire output is `Error: "missing environment variable
    // SMARTME_GROUP_ID"`, with nothing saying which build produced it.
    //
    // `CARGO_PKG_VERSION` is resolved at COMPILE time, so it describes the binary
    // rather than the image tag it happens to be wearing. Those can drift — the
    // publish workflow carries a tag-vs-version guard precisely because they can
    // — and when they do, this line is the one that is right.
    //
    // `CONTRACT_VERSION` is here because it, not the package version, is what a
    // consumer sees: it says what the wire will carry, and it is the first thing
    // worth knowing when a tag looks wrong in Ignition.
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        contract = smartme_bridge::adapters::sparkplug_publisher::CONTRACT_VERSION,
        "smartme_mqtt starting"
    );

    if let Some(log) = &file_log {
        tracing::info!(directory = %log.directory.display(), "logging to file");
    }

    // Read, then validate — and nothing in between. Every rule lives in
    // `app::config`, because FR46 gives the configuration a SECOND consumer (the
    // web UI) and two places applying the same rules is how they drift apart.
    let raw = RawConfig {
        api_base: std::env::var("SMARTME_API_BASE").ok(),
        client_id: std::env::var("SMARTME_CLIENT_ID").ok(),
        client_secret: std::env::var("SMARTME_CLIENT_SECRET").ok(),
        group_id: std::env::var("SMARTME_GROUP_ID").ok(),
        node_id: std::env::var("SMARTME_NODE_ID").ok(),
        broker_host: std::env::var("SMARTME_BROKER_HOST").ok(),
        broker_port: std::env::var("SMARTME_BROKER_PORT").ok(),
        state_dir: std::env::var("SMARTME_STATE_DIR").ok(),
        publish_period_secs: std::env::var("SMARTME_PUBLISH_PERIOD_SECS").ok(),
        // ONE meter from the environment, deliberately.
        //
        // The model holds any number (Story 5.1) so the configuration screen can
        // be built against its final shape, but the environment is the BOOTSTRAP
        // path (FR23) and inventing an indexed variable scheme here would be a
        // second configuration surface to keep in step with the first. More
        // meters arrive through the UI, which is Epic 6.
        meters: vec![RawMeter {
            meter_id: std::env::var("SMARTME_METER_ID").ok(),
            device_id: std::env::var("SMARTME_DEVICE_ID").ok(),
            serial: std::env::var("SMARTME_SERIAL").ok(),
            enabled: None,
        }],
    };

    let config = match smartme_bridge::app::config::validate(raw) {
        Ok(config) => config,
        Err(errors) => {
            // Every fault at once, and to stderr as well as the log: a first run
            // that fails here may have no log destination configured yet, and an
            // operator who cannot see why is an operator who guesses.
            tracing::error!("{errors}");
            eprintln!("{errors}");
            return Err(errors.to_string().into());
        }
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
