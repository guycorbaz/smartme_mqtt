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
use crate::app::mqtt_driver::{self, DeviceCommand, MqttConfig};
use crate::app::poll_publish::{self, Heartbeats, PollConfig};
use crate::app::reconfigure::{self, Cost, Plan};
use crate::core::clock::{Clock, SystemClock};
use crate::core::state_machine::Policy;
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
    /// Every meter configured, enabled or not.
    ///
    /// **Model and runtime are both plural since Story 3.1.** The model went
    /// first (Story 5.1), so the configuration screen could be built against its
    /// final shape rather than reshaped — with its form — when the runtime caught
    /// up; `RUNTIME_METER_LIMIT` refused a configuration enabling more than the
    /// runtime served in the meantime, so a subset was never published silently.
    /// Every enabled meter is now served, so there is no subset to refuse.
    pub meters: Vec<crate::app::config::MeterConfig>,
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
    /// Where rotated log files go, if anywhere. Absent means console only.
    ///
    /// **Carried here but not used by the runtime.** The tracing subscriber is
    /// built in `main.rs` before this struct exists, and cannot be re-pointed
    /// afterwards. It is part of the configuration all the same, because a
    /// setting the model cannot see is a setting a reload cannot notice changed
    /// — and [`crate::app::reconfigure`] has to be able to tell an operator that
    /// this one needs a restart rather than silently doing nothing.
    pub log_dir: Option<String>,
    /// How many rotated files to keep. Same caveat as [`Self::log_dir`].
    pub log_keep: Option<usize>,
    /// Port the embedded web UI listens on (Story 6.1).
    pub ui_port: Option<u16>,
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
    /// No meter is enabled.
    ///
    /// Unreachable through [`crate::app::config::validate`], which reports it as
    /// a configuration fault along with everything else that is wrong. It exists
    /// because a `BridgeConfig` can also be built by hand — every chaos test does
    /// — and a hand-built one with no enabled meter would otherwise panic here or,
    /// worse, run and publish nothing.
    #[error("no meter is enabled; the bridge would connect, birth, and publish nothing")]
    NoEnabledMeter,
}

/// Bound of the reconfiguration channel.
///
/// Small on purpose. A reconfiguration is an operator pressing Save, not a
/// stream — anything that queued more than this would be a loop, and a bound
/// that hides a loop is worse than one that reveals it.
const DEVICE_QUEUE: usize = 8;

/// The configuration in force, swappable without stopping anything (AR8).
///
/// `ArcSwap` rather than a lock, deliberately: the poll loop reads this on every
/// tick and the driver reads it from another task, so a writer that blocked
/// readers would put a web form on the critical path of the publish loop.
pub type ConfigHandle = Arc<arc_swap::ArcSwap<BridgeConfig>>;

/// The live control surface — what a reconfiguration needs in order to act
/// (Story 5.2 AC4).
///
/// Held by whoever can change the configuration. Today that is a test; from
/// Epic 6 it is the web UI. Nothing here knows about HTTP, which is the whole
/// point of the Epic 5 / Epic 6 split.
#[derive(Clone)]
pub struct Control {
    config: ConfigHandle,
    devices: mpsc::Sender<DeviceCommand>,
    heartbeats: Heartbeats,
    /// The sink's health (story 6.5). Handed to the UI the same way the heartbeats
    /// are: a read-only view of what the driver observed.
    sink: crate::app::mqtt_driver::SinkHealth,
    clock: Arc<dyn Clock + Send + Sync>,
}

#[cfg(test)]
impl Control {
    /// A control surface whose device commands go nowhere, for tests that need
    /// a [`Control`] to read from rather than to act through.
    ///
    /// **Test-only and crate-private on purpose.** A `Control` that silently
    /// drops births and deaths is exactly the object this project must never
    /// hand to production code: it would report a device certificate as sent
    /// while nothing reached the wire.
    /// As [`detached`](Self::detached), with a sink the caller can drive — story
    /// 6.5's tests need to observe a connect and a loss, and a helper that could
    /// only build a never-connected sink would make [#53]'s whole point untestable.
    pub(crate) fn detached_with_sink(
        config: ConfigHandle,
        heartbeats: Heartbeats,
        clock: Arc<dyn Clock + Send + Sync>,
        sink: crate::app::mqtt_driver::SinkHealth,
    ) -> Self {
        let mut control = Self::detached(config, heartbeats, clock);
        control.sink = sink;
        control
    }

    pub(crate) fn detached(
        config: ConfigHandle,
        heartbeats: Heartbeats,
        clock: Arc<dyn Clock + Send + Sync>,
    ) -> Self {
        let (devices, receiver) = mpsc::channel(1);
        // Kept alive, so a send fails by filling rather than by closing — the
        // failure mode a real driver has.
        std::mem::forget(receiver);
        Self {
            config,
            devices,
            heartbeats,
            // A detached control has never connected, which is the honest answer
            // for a surface that is being read rather than driven.
            sink: crate::app::mqtt_driver::SinkHealth::new(),
            clock,
        }
    }
}

impl Control {
    /// The configuration **in force**.
    ///
    /// Not "the configuration the operator last saved" — those are two different
    /// things and conflating them is how a screen comes to report a setting that
    /// is not doing anything. `config.toml` holds what is *desired*; this holds
    /// what the running process is *actually using*. [`Self::apply`] moves a
    /// value from the first to the second only when it really took effect, and
    /// says in its return value what it left behind.
    /// The poll loop's heartbeat (AR12).
    ///
    /// Handed out rather than duplicated: `/healthz` must report the SAME
    /// instant the loop records, or a healthcheck would be acting on a second
    /// opinion about whether the bridge is alive.
    /// What the driver last observed of the broker, or `None` if it has never
    /// connected — which is not the same as disconnected (story 6.5 AC4).
    pub fn sink(&self) -> Option<crate::app::mqtt_driver::SinkState> {
        self.sink.state()
    }

    pub fn heartbeats(&self) -> Heartbeats {
        self.heartbeats.clone()
    }

    /// The clock the heartbeat is recorded against.
    ///
    /// Handed out rather than reconstructed: `MonotonicMs` counts from a
    /// process-start instant held INSIDE the clock, so a second `SystemClock`
    /// would be a second origin and every age computed against it would be
    /// wrong by the difference.
    pub fn clock(&self) -> Arc<dyn Clock + Send + Sync> {
        Arc::clone(&self.clock)
    }

    /// The live configuration, for a reader that needs the publish period.
    pub fn config_handle(&self) -> ConfigHandle {
        Arc::clone(&self.config)
    }

    pub fn current(&self) -> Arc<BridgeConfig> {
        self.config.load_full()
    }

    /// Apply a new configuration to the running bridge, and report what it cost.
    ///
    /// Only changes this process can actually carry out are stored. A field
    /// classified [`Cost::NewSession`] or [`Cost::ProcessRestart`] is left as it
    /// was, and named in the returned [`Plan`] — so a caller can tell the
    /// operator *"saved, and it takes effect when you restart"* instead of
    /// *"saved"*, which would be a claim the bridge did not honour.
    ///
    /// The new configuration must already have been through
    /// [`crate::app::config::validate`]; a `BridgeConfig` cannot be obtained any
    /// other way except by hand.
    pub async fn apply(&self, new: BridgeConfig) -> Plan {
        let old = self.current();
        // The meters with a poll task, from the tasks themselves — not inferred
        // from either configuration. See `reconfigure::classify_meters`: the
        // inference was right for a one-meter runtime and silently wrong from
        // the day story 3.1 served the fleet.
        let served = self.heartbeats.meters();
        let mut plan = reconfigure::classify(&old, &new, &served);
        if plan.is_empty() {
            return plan;
        }

        // Certificates FIRST, and deaths before births.
        //
        // A death that arrived after its replacement's birth would leave the host
        // holding a bury for a device that had just been announced. The driver
        // publishes each one inline, so their order on the wire is this order.
        // The send result is CHECKED, not discarded.
        //
        // It was `let _ =` until 2026-08-05, so a closed or dead driver produced
        // a plan claiming certificates that never reached the broker — the host
        // would go on showing a buried device's last value as current. Whatever
        // is not delivered is named in the plan, and the caller renders it.
        let mut undelivered = Vec::new();
        for serial in &plan.deaths {
            if self
                .devices
                .send(DeviceCommand::Death(serial.clone()))
                .await
                .is_err()
            {
                undelivered.push(serial.clone());
            }
        }
        for serial in &plan.births {
            if self
                .devices
                .send(DeviceCommand::Birth(serial.clone()))
                .await
                .is_err()
            {
                undelivered.push(serial.clone());
            }
        }
        if !undelivered.is_empty() {
            tracing::error!(
                ?undelivered,
                "device certificates were NOT delivered to the driver; the broker \
                 has not been told, and a host will keep showing what it last saw"
            );
        }
        plan.undelivered = undelivered;

        // Then the swap — but ONLY of what is genuinely in force now.
        //
        // `api_base` and `http_timeout` were stored here until 2026-08-04 and
        // were never in force: the client that holds them is built once, before
        // the poll task exists. Storing them made `current()` report an endpoint
        // the bridge was not using. The meter list is stored because the DEVICE
        // SET really does change (the certificates above are how), but a meter's
        // `device_id` is carried by the source and is not — so the stored list
        // keeps the old `device_id`s, and `classify` reports that field as
        // `ProcessRestart` so nobody is told otherwise. [#52].
        let mut applied = (*old).clone();
        applied.meters = new
            .meters
            .iter()
            .map(|m| {
                let mut m = m.clone();
                if let Some(previous) = old.meters.iter().find(|o| o.meter == m.meter) {
                    m.device_id = previous.device_id.clone();
                }
                m
            })
            .collect();
        applied.poll = new.poll;
        applied.policy = new.policy;
        self.config.store(Arc::new(applied));

        let held_back = plan.needs_restart();
        if !held_back.is_empty() {
            tracing::warn!(
                fields = ?held_back,
                "saved but NOT in force: these take effect at the next process start"
            );
        }
        // `any`, not `cost()`. `cost()` is the MAXIMUM, and `ProcessRestart`
        // outranks `NewSession` — so a form that changed the broker AND a log
        // setting reported only the log setting and said nothing about the
        // broker change it had also discarded. A "Save" button produces exactly
        // that mixed plan. Found by review 2026-08-04.
        if plan.changes.iter().any(|c| c.cost == Cost::NewSession) {
            tracing::warn!(
                "saved but NOT in force: the Sparkplug identity or the broker changed, \
                 which is a new session by definition. The bridge keeps the session it \
                 has until it is restarted"
            );
        }
        plan
    }
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
    run_with_control(config, shutdown, |_| {}).await
}

/// As [`run`], handing the caller the live [`Control`] before the tasks start.
///
/// Split out for one reason: AC4's mechanism has no caller until Epic 6 builds
/// the screens, and a mechanism with no caller is a mechanism nothing exercises.
/// `with_control` is how a test — and later the web server — gets hold of it
/// without `run`'s signature growing a parameter every caller must ignore.
pub async fn run_with_control(
    config: BridgeConfig,
    shutdown: impl Future<Output = ()> + Send + 'static,
    with_control: impl FnOnce(Control),
) -> Result<(), StartupError> {
    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(config.group_id.clone(), config.node_id.clone())?;
    // EVERY enabled meter is served (Story 3.1). Until 2026-08-06 this took the
    // first one and `config::validate` refused a configuration enabling more, so
    // that it was a guaranteed single rather than a silent truncation. The guard
    // was right and has been outgrown, not weakened: the truncation it forbade is
    // still forbidden, because nothing is dropped here at all.
    let served: Vec<_> = config
        .meters
        .iter()
        .filter(|meter| meter.enabled)
        .cloned()
        .collect();
    if served.is_empty() {
        return Err(StartupError::NoEnabledMeter);
    }
    // Validate every device identifier HERE, and ALL of them before any task
    // starts: a serial that cannot be a topic level would otherwise leave the
    // node connected, unborn and publishing nothing, forever. Refusing to start
    // beats starting wrong — and refusing on the FOURTH meter after three are
    // already polling would be starting wrong.
    for meter in &served {
        node.device_topic(sparkplug_b::MessageType::DBirth, meter.serial.as_str())?;
    }

    let client = SmartMeClient::new(
        config.api_base.clone(),
        config.credentials.clone(),
        config.http_timeout,
    )?;

    // The channel first: the seam exists before either end does.
    //
    // **SINCE STORY 4.11 THIS BOUND IS A LOSS THRESHOLD**, and it carried no
    // comment at all until the review said so. The driver does not drain this
    // inbox while it sleeps between reconnect attempts, so 64 is how many readings
    // an outage may accumulate before `try_send` starts refusing them and counting
    // `DropReason::OutboxFull` — about ten minutes at three meters on a 30 s
    // period, and it shrinks as meters × rate grows. Raising it does not save a
    // reading: AR7 forbids a buffer, and a larger number is a buffer with a
    // different name. It is stated here so the next reader knows what the figure
    // decides.
    let (tx, rx) = mpsc::channel(64);
    let heartbeats = Heartbeats::for_meters(served.iter().map(|m| m.meter.clone()));
    // The sink's health, observed by the driver and read by every surface (story
    // 6.5, [#53]). Created here beside the meters' so the two reach `Control`
    // together — FR29 asks for them independently, which means both must exist.
    let sink = crate::app::mqtt_driver::SinkHealth::new();
    let (death_tx, death_rx) = oneshot::channel();
    // Reconfiguration gets its OWN channel, for the same reason inbound commands
    // do: sharing the reading path would put an externally-driven, bursty
    // sender behind a bound sized for readings.
    let (device_tx, device_rx) = mpsc::channel(DEVICE_QUEUE);
    let handle: ConfigHandle = Arc::new(arc_swap::ArcSwap::from_pointee(config.clone()));
    with_control(Control {
        config: Arc::clone(&handle),
        devices: device_tx.clone(),
        heartbeats: heartbeats.clone(),
        sink: sink.clone(),
        clock: Arc::clone(&clock),
    });

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
        served.iter().map(|m| m.serial.clone()).collect(),
        Arc::clone(&clock),
        crate::app::mqtt_driver::Health {
            meters: heartbeats.clone(),
            sink: sink.clone(),
        },
        rx,
        device_rx,
        death_rx,
    ));

    // ONE TASK PER METER, not one task walking the meters.
    //
    // The fetch carries its own timeout (10 s by default). A single task walking
    // four meters would serialise four timeouts — 40 s inside a 30 s period — so
    // one unreachable meter would push every other meter's poll past its own
    // deadline. That is FR12 failing by construction, and NFR2's bound
    // unmeetable for reasons having nothing to do with the meter it is measured
    // on. One of the author's four meters is physically unplugged, so this is the
    // normal case here rather than the unlucky one.
    //
    // What stays singular is what the norm makes singular: the sequence number
    // and the transport, both behind the one driver task (AR6).
    let polls: Vec<_> = served
        .iter()
        .map(|meter| {
            // The serial goes down with the meter, and it is the SAME value
            // `node.device_topic` above births the device under. That is the
            // point: the source compares it with what smart-me answers, so a
            // serial that is legal but wrong stops the meter loudly instead of
            // producing a bridge that fetches, judges and ticks while the
            // publisher discards every reading (`UnverifiedReading::verify`).
            let source = SmartMeCloudSource::new(
                client.clone(),
                Arc::clone(&clock),
                meter.meter.clone(),
                meter.serial.clone(),
                meter.device_id.clone(),
            );
            tokio::spawn(poll_publish::run(
                poll_publish::PolledMeter {
                    meter: meter.meter.clone(),
                    // Captured at spawn: the serial the DBIRTH below uses, so
                    // every certificate this task sends names the IN-FORCE
                    // device rather than a stored, not-yet-restarted edit.
                    serial: meter.serial.clone(),
                },
                source,
                Arc::clone(&clock),
                Arc::clone(&handle),
                heartbeats.clone(),
                tx.clone(),
                // The state directory: the same one `bd_seq_path` lives in, so
                // one `chown` covers everything the bridge persists.
                config
                    .bd_seq_path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .to_path_buf(),
                device_tx.clone(),
            ))
        })
        .collect();
    // The template sender is dropped so the channel closes when the LAST poll
    // task stops, rather than staying open on a sender nobody sends through.
    drop(tx);

    shutdown.await;
    tracing::info!("shutdown signalled");

    // Tell the driver to publish the death, THEN stop the poll tasks: the order
    // matters only in that the death must not be blocked behind a pending
    // reading.
    let _ = death_tx.send(());
    for poll in &polls {
        poll.abort();
    }
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
    use crate::domain::{MeterId, Serial};

    /// A [`Control`] with no bridge behind it: the channel is the observation
    /// point, so what `apply` decides is visible without a broker.
    /// The heartbeats are built FROM THE CONFIGURATION, the way
    /// [`run_with_control`] builds them — one per enabled meter.
    ///
    /// It said `Heartbeats::for_meters(["meter-a"])` until 2026-08-08 while
    /// `config()` describes a meter called `garage`: a harness asserting a served
    /// set that had nothing to do with its own configuration. Nothing could
    /// notice, because `classify` inferred the served meter from the config and
    /// never looked at the heartbeats. Making the real code ask the tasks made
    /// this test fail — `Some(ProcessRestart)` where a certificate was owed —
    /// which is the harness admitting what it had been modelling.
    fn control() -> (Control, mpsc::Receiver<DeviceCommand>) {
        let (devices, rx) = mpsc::channel(8);
        let started = config();
        (
            Control {
                config: Arc::new(arc_swap::ArcSwap::from_pointee(config())),
                devices,
                heartbeats: Heartbeats::for_meters(
                    started
                        .meters
                        .iter()
                        .filter(|m| m.enabled)
                        .map(|m| m.meter.clone()),
                ),
                sink: crate::app::mqtt_driver::SinkHealth::new(),
                clock: Arc::new(crate::core::clock::SystemClock::new()),
            },
            rx,
        )
    }

    /// AC4's heart, at the level where the decision is made.
    #[tokio::test]
    async fn disabling_a_meter_buries_it_and_enabling_it_births_it() {
        let (control, mut devices) = control();

        let mut off = config();
        off.meters[0].enabled = false;
        let plan = control.apply(off.clone()).await;
        assert_eq!(plan.cost(), Some(Cost::DeviceCertificate));
        assert_eq!(
            devices.try_recv().expect("a certificate was owed"),
            DeviceCommand::Death(Serial::new("30000001"))
        );

        let plan = control.apply(config()).await;
        assert_eq!(plan.cost(), Some(Cost::DeviceCertificate));
        assert_eq!(
            devices.try_recv().expect("a certificate was owed"),
            DeviceCommand::Birth(Serial::new("30000001"))
        );
    }

    /// The period is genuinely in force after `apply`, with nothing disturbed.
    ///
    /// FALSIFIED 2026-08-04 by dropping `applied.poll = new.poll` from `apply`:
    /// the plan still said `Hot` and `current()` still said 30 s.
    #[tokio::test]
    async fn the_publish_period_is_in_force_immediately_and_costs_no_certificate() {
        let (control, mut devices) = control();
        let mut faster = config();
        faster.poll.interval = Duration::from_secs(15);

        assert_eq!(control.apply(faster).await.cost(), Some(Cost::Hot));
        assert_eq!(control.current().poll.interval, Duration::from_secs(15));
        assert!(
            devices.try_recv().is_err(),
            "changing the period must not disturb a single device"
        );
    }

    /// **`current()` describes what is IN FORCE, never what was merely saved.**
    ///
    /// FALSIFIED 2026-08-04 by storing `broker_host` and `log_dir` in `apply`
    /// anyway — the change someone makes when "apply the new config" reads as
    /// "store the new config". Both this test and
    /// `a_log_setting_is_named_as_needing_a_restart_and_not_applied` went red.
    ///
    /// A screen that reads back the new broker would tell the operator the bridge
    /// had moved when it had not. The file holds what is desired; this holds what
    /// is running, and the plan is what carries the difference between them.
    #[tokio::test]
    async fn a_change_that_needs_a_new_session_is_reported_and_not_pretended() {
        let (control, _devices) = control();
        let mut elsewhere = config();
        elsewhere.broker_host = "192.0.2.9".to_string();

        let plan = control.apply(elsewhere).await;
        assert_eq!(plan.cost(), Some(Cost::NewSession));
        assert_eq!(
            control.current().broker_host,
            "127.0.0.1",
            "the bridge is still connected to the old broker, so that is what \
             current() must say"
        );
    }

    /// **The lie a review found on 2026-08-04.** `api_base` was classified `Hot`
    /// and stored by `apply`, so `current()` reported an endpoint the bridge was
    /// not using — the client that holds it is built once, before the poll task
    /// exists, and nothing rebuilds it.
    ///
    /// This test is the regression guard, and it pins the HONEST behaviour: the
    /// change is reported as needing a restart, and `current()` keeps saying what
    /// is actually in force. [#52] is where it becomes genuinely hot.
    #[tokio::test]
    async fn a_field_the_bridge_cannot_adopt_is_never_reported_as_in_force() {
        let (control, _devices) = control();
        let mut elsewhere = config();
        elsewhere.api_base = "https://other.example.com".to_string();
        elsewhere.http_timeout = Duration::from_secs(42);

        let plan = control.apply(elsewhere).await;
        assert_eq!(plan.cost(), Some(Cost::ProcessRestart));
        assert_eq!(
            plan.needs_restart(),
            vec!["api_base", "http_timeout"],
            "the operator must be told which fields are waiting, by name"
        );
        assert_eq!(
            control.current().api_base,
            "https://api.smart-me.com",
            "every request still goes to the old endpoint, so that is what \
             current() must say"
        );
        assert_eq!(
            control.current().http_timeout,
            Duration::from_secs(10),
            "the client holding this was built before the poll task existed"
        );
    }

    /// A mixed save must not swallow the new-session warning.
    ///
    /// FOUND BY REVIEW 2026-08-04: the warning was gated on `plan.cost()`, which
    /// is the MAXIMUM, and `ProcessRestart` outranks `NewSession`. So changing the
    /// broker and a log setting together reported only the log setting — and a
    /// "Save" button is precisely what produces a mixed plan.
    #[tokio::test]
    async fn a_mixed_save_still_reports_the_new_session_change() {
        let (control, _devices) = control();
        let mut both = config();
        both.broker_host = "192.0.2.9".to_string();
        both.log_dir = Some("/data/logs".to_string());

        let plan = control.apply(both).await;
        assert_eq!(
            plan.cost(),
            Some(Cost::ProcessRestart),
            "the maximum is still ProcessRestart — that part was right"
        );
        assert!(
            plan.changes
                .iter()
                .any(|c| c.cost == Cost::NewSession && c.field == "broker_host"),
            "and the broker change must still be IN the plan, or the caller \
             cannot tell the operator it was discarded: {plan:?}"
        );
        assert_eq!(control.current().broker_host, "127.0.0.1");
    }

    /// Same rule, for the category that cannot be applied at all.
    #[tokio::test]
    async fn a_log_setting_is_named_as_needing_a_restart_and_not_applied() {
        let (control, _devices) = control();
        let mut logged = config();
        logged.log_dir = Some("/data/logs".to_string());

        let plan = control.apply(logged).await;
        assert_eq!(plan.needs_restart(), vec!["log_dir"]);
        assert_eq!(
            control.current().log_dir,
            None,
            "the subscriber was built before this struct existed and cannot be \
             re-pointed; claiming otherwise would make a form report a success \
             the bridge never delivered"
        );
    }

    fn config() -> BridgeConfig {
        BridgeConfig {
            api_base: "https://api.smart-me.com".to_string(),
            credentials: Credentials::Basic {
                user: "u".to_string(),
                password: "p".to_string(),
            },
            http_timeout: Duration::from_secs(10),
            meters: vec![crate::app::config::MeterConfig {
                meter: MeterId::new("garage"),
                device_id: "a1a1a1a1-b2b2-c3c3-d4d4-000000000001".to_string(),
                serial: Serial::new("30000001"),
                enabled: true,
            }],
            group_id: "Site".to_string(),
            node_id: "Bridge".to_string(),
            broker_host: "127.0.0.1".to_string(),
            broker_port: 1883,
            // Per test binary, not a fixed name: this path's PARENT is also the
            // state directory every poll task writes its monotonicity reference
            // into (`run_with_control`), so a fixed one puts two concurrent
            // `cargo test` runs on the same reference files. Same reasoning as
            // `poll_publish::tests::scratch_dir`, 2026-08-12.
            bd_seq_path: std::env::temp_dir()
                .join(format!("smartme_supervisor_{}", std::process::id()))
                .join("bdseq.toml"),
            poll: PollConfig {
                interval: Duration::from_secs(5),
                fetch_timeout: Duration::from_secs(2),
            },
            policy: Policy::DEFAULT,
            log_dir: None,
            log_keep: None,
            ui_port: None,
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
