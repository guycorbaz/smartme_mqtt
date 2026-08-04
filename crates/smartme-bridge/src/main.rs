//! Thin shell over `lib::run()` — the wiring lives in the library so the
//! integration and chaos tests can drive the same code the binary runs.
//!
//! Configuration is read from `config.toml` into `app::config::RawConfig` and
//! validated by `app::config::validate` — this file applies no rule of its own.
//!
//! **The file is the configuration** ([ADR 0023]). The environment carries two
//! things and no others: `SMARTME_STATE_DIR`, because a file cannot say where it
//! lives, and the smart-me credential, which never descends to disk. The domains
//! are disjoint, so nothing here merges or arbitrates between them.
//!
//! Secrets are never logged and never reach a fault message (NFR12, ADR 0019).
//!
//! [ADR 0023]: ../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md

use std::path::PathBuf;

use smartme_bridge::app::store::{self, Credential, StoredConfig};
use smartme_bridge::ui;

/// Where the state directory lives when `SMARTME_STATE_DIR` does not say.
const DEFAULT_STATE_DIR: &str = "/data";

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
/// **Opt-in, deliberately.** File logging exists only when `log_dir` is set in
/// the configuration. With it unset the binary behaves exactly as it did before
/// this was added — which is what the two chaos tests that spawn the real binary
/// and read its output depend on. A default path would have made a comfort
/// feature able to break the tests that guard the product's honesty.
///
/// **Takes the stored configuration, and takes it before validation.** `log_dir`
/// and `log_keep` moved out of the environment on 2026-08-04 ([ADR 0023]), which
/// inverted the order this file used to run in: logging can no longer be set up
/// before the configuration is read, because the configuration is where the log
/// settings are. `None` here covers both the first run and an unreadable file —
/// in either case the console is the only destination, which is exactly when it
/// matters that faults also go to `stderr`.
///
/// **Never fatal.** A bridge that stops publishing because it cannot write a log
/// file has turned a diagnostic aid into an outage. Every failure here degrades
/// to console-only and says so on stderr — loudly, because the failure this
/// whole change was requested to diagnose (`bdSeq`, `Permission denied`) was one
/// nobody saw until they went looking for a file that was never there.
fn file_log_layer<S>(
    stored: Option<&StoredConfig>,
) -> (
    Option<Box<dyn tracing_subscriber::Layer<S> + Send + Sync>>,
    Option<FileLog>,
)
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    use tracing_subscriber::Layer as _;

    let Some(stored) = stored else {
        return (None, None);
    };
    let Some(directory) = stored.log_dir.as_ref() else {
        return (None, None);
    };
    let directory = PathBuf::from(directory);

    let keep = match stored.log_keep {
        Some(n) if n > 0 => n,
        Some(n) => {
            eprintln!(
                "log_keep is {n}, which would keep no history at all; \
                 keeping {} files",
                store::DEFAULT_LOG_KEEP
            );
            store::DEFAULT_LOG_KEEP
        }
        None => store::DEFAULT_LOG_KEEP,
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

    // PHASE 1 — read the file, before there is anywhere to report about it.
    //
    // The order is forced, not chosen: `log_dir` and `log_keep` live in the
    // configuration, so the configuration must be read before the subscriber can
    // be built. Nothing is traced or printed here; the outcome is held and
    // reported in phase 3, once there is a log to report it to.
    //
    // `exists` separates two states that must not be merged. Absent is a first
    // run — the bridge comes up and serves the UI. Present-but-unreadable is a
    // refusal to start. Merged, this either bricks the first run or treats a
    // corrupt file as a fresh install and overwrites it.
    let state_dir = PathBuf::from(
        std::env::var("SMARTME_STATE_DIR").unwrap_or_else(|_| DEFAULT_STATE_DIR.to_string()),
    );
    let stored: Option<Result<StoredConfig, _>> =
        store::exists(&state_dir).then(|| store::read(&state_dir));

    // PHASE 2 — logging, configured by what phase 1 found, if anything.
    // The UI's port comes from the file when there is one, and from a default
    // when there is not — because the run with no file is the run that needs the
    // UI most, and it has nowhere to read a port from.
    let ui_port = stored
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|c| c.ui_port)
        .unwrap_or(ui::DEFAULT_PORT);

    let (file_layer, file_log) = file_log_layer(stored.as_ref().and_then(|r| r.as_ref().ok()));
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
    // It is emitted before anything that can be *reported*, and therefore before
    // any refusal to start. A banner printed after the configuration faults would
    // be absent from the one log an operator most needs it in: the crash-looping
    // container, whose entire output would otherwise be a complaint about a
    // setting, with nothing saying which build is doing the complaining.
    //
    // Note what changed on 2026-08-04 and what did not. The configuration is now
    // READ before this line, because the log settings are in it — but nothing is
    // *said* before this line, which is the property that was ever worth having.
    // A failure between the two reaches stderr and nowhere else, which is why
    // phase 1 does no reporting of its own.
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

    // PHASE 3 — act on what phase 1 found, now that it can be said out loud.
    //
    // Every fault goes to stderr as well as the log. That is not belt-and-braces:
    // a run that fails here may have no file destination configured, and reaching
    // an operator who is watching `docker compose logs` is the whole point of
    // saying anything at all.
    let outcome = match stored {
        // A first run. Not a fault, and not reported as one — the operator has
        // done nothing wrong, they have simply not done it yet.
        None => {
            tracing::info!(
                path = %store::config_path(&state_dir).display(),
                "no configuration yet — the bridge is up and will publish nothing \
                 until one exists. Configure it in the web UI; it is written here"
            );
            smartme_bridge::run_without_publishing(Some((ui_port, ui::Lifecycle::Unconfigured)))
        }

        // A configuration that exists and cannot be read. Refusing beats starting
        // on defaults nobody chose (Story 5.1).
        //
        // AND THE UI IS NOT SERVED HERE — decided in Story 6.1 AC1, not left to
        // be discovered. The refusal is FR26's whole point; the faults already
        // reach `docker compose logs` and stderr without a browser; and a bridge
        // that stayed up on a configuration it had REJECTED is one restart away
        // from somebody believing it is running. Serving a repair page would also
        // need the listener up before the configuration is read, inverting the
        // ordering this file was given for the log settings.
        //
        // The documented repair is to fix the file. The manual says so.
        Some(Err(errors)) => {
            tracing::error!("{errors}");
            eprintln!("{errors}");
            return Err(errors.to_string().into());
        }

        // A configuration that exists and parses. The credential joins it here,
        // and only here — the two sources have no field in common, so this is a
        // join and never a merge (ADR 0023 §4).
        Some(Ok(stored)) => {
            let confirmed = stored.mapping_confirmed;
            let credential = Credential {
                client_id: std::env::var("SMARTME_CLIENT_ID").ok(),
                client_secret: std::env::var("SMARTME_CLIENT_SECRET").ok(),
            };
            let raw = store::into_raw(stored, credential, &state_dir);

            // Read, then validate — and nothing in between. Every rule lives in
            // `app::config`, because FR46 gives the configuration a SECOND
            // consumer (the web UI), and two places applying the same rules is
            // how they drift apart.
            match smartme_bridge::app::config::validate(raw) {
                // Valid, and nobody has said the mapping is right (Story 5.3).
                //
                // NO SESSION AT ALL, not merely no DDATA: an NBIRTH on its own
                // creates the node's folder in a Sparkplug host's tag tree, and
                // that folder outlives the process and is deleted by hand.
                // Withholding only the data would leave the irreversible half
                // already done.
                //
                // This is the guard against the one error validation cannot see
                // — a mapping that is well-formed, unique, complete and pointed
                // at the wrong meter.
                Ok(_) if !confirmed => {
                    tracing::warn!(
                        path = %store::config_path(&state_dir).display(),
                        "the meter mapping has NOT been confirmed, so nothing is \
                         published — no connection, no birth. Check each meter's \
                         serial against its topic in the web UI and confirm it; \
                         a mapping that is merely well-formed can still be wrong, \
                         and a SCADA host keeps the tags it discovers"
                    );
                    smartme_bridge::run_without_publishing(Some((
                        ui_port,
                        ui::Lifecycle::Unconfirmed,
                    )))
                }
                Ok(config) => smartme_bridge::run(config, Some(ui_port)),
                Err(errors) => {
                    tracing::error!("{errors}");
                    eprintln!("{errors}");
                    return Err(errors.to_string().into());
                }
            }
        }
    };

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
