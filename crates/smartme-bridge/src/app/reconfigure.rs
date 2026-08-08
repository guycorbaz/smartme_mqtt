//! What a configuration change costs, decided per field and enforced by the
//! compiler (Story 5.2 AC4, AR8).
//!
//! # The rule this module exists to make unbreakable
//!
//! The story says it plainly: *"no field may be left undecided — an unlisted
//! field will be assumed hot by whoever adds the form."* A table in a document
//! cannot enforce that; the next field added would simply not appear in it, and
//! nothing would complain.
//!
//! So [`classify`] **destructures both configurations exhaustively, with no
//! `..` rest pattern.** Adding a field to [`BridgeConfig`] stops the build until
//! somebody says what changing it costs. The decision is still a human one — the
//! compiler only guarantees that it is taken.
//!
//! # The four costs, named for what the OPERATOR sees
//!
//! Not for what the code does. "Restarting the poll task" and "rebuilding the
//! HTTP client" are the same event to someone watching Ignition — nothing —
//! so they are one category here.
//!
//! | cost | what it looks like from outside |
//! | --- | --- |
//! | [`Cost::Hot`] | nothing. The change is simply in force. |
//! | [`Cost::DeviceCertificate`] | a DDEATH and/or DBIRTH for **one** device, same session, same `bdSeq`, other devices untouched |
//! | [`Cost::NewSession`] | the node disconnects and comes back: NDEATH, a new `bdSeq`, NBIRTH |
//! | [`Cost::ProcessRestart`] | nothing at all until the container is restarted |
//!
//! # Why `ProcessRestart` exists rather than being engineered away
//!
//! `log_dir` and `log_keep` build the tracing subscriber, which is installed
//! once, before the first line is logged, and cannot be re-pointed afterwards
//! without a reloadable layer. That is buildable. It is not built, and the
//! honest answer is to say so in a category the UI must show — because the
//! failure mode of pretending otherwise is a form that accepts a new log
//! directory, reports success, and writes nowhere.
//!
//! The smart-me credential is `ProcessRestart` for a different reason: it is not
//! in the file at all ([ADR 0023]), so no reload can carry it.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md

use crate::app::config::MeterConfig;
use crate::app::supervisor::BridgeConfig;
use crate::domain::Serial;

/// What applying a change costs, in the terms an operator experiences.
///
/// Ordered least to most disruptive. [`Plan`] keeps the **maximum** — a change
/// that touches both the period and the broker costs a new session, and saying
/// "hot" about any part of it would be a half-truth an operator acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cost {
    /// In force immediately, invisible from outside.
    Hot,
    /// One device is re-announced. Same session, same `bdSeq`.
    DeviceCertificate,
    /// The node reconnects: NDEATH, new `bdSeq`, NBIRTH.
    NewSession,
    /// Cannot be applied to a running process at all.
    ProcessRestart,
}

/// One field that changed, and what it costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The setting, named as the operator knows it — the same vocabulary the
    /// faults use, so a form can show one next to the other.
    pub field: &'static str,
    pub cost: Cost,
}

/// Everything that changed between two configurations, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Every field that differs, **grouped by cost** — hot first, then new
    /// session, then process restart, then the meter set.
    ///
    /// It said *"in declaration order"* until a review on 2026-08-04 pointed out
    /// that it never was: the meters are compared last while being declared
    /// fourth. Grouping is what the code actually does and what a form wants,
    /// since it renders by consequence rather than by struct layout.
    pub changes: Vec<Change>,
    /// Devices to announce, **before** any DDATA can carry their metrics.
    pub births: Vec<Serial>,
    /// Devices to bury.
    pub deaths: Vec<Serial>,
    /// Certificates the driver did NOT accept.
    ///
    /// **Empty is the claim, not the default.** `Control::apply` used to discard
    /// the result of every device send with `let _ =`, so a plan could report
    /// *"one device re-announced"* while nothing reached the wire — which is
    /// verbatim what `Control::detached`'s own documentation says must never be
    /// handed to production code. A screen that says a device was buried when the
    /// bury was dropped is the SIGTERM-NO-LIE failure with a different door.
    pub undelivered: Vec<Serial>,
}

impl Plan {
    /// Nothing differs.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The most disruptive cost in the plan, or `None` when nothing changed.
    ///
    /// **The maximum, not a list.** An operator deciding whether to press Save
    /// needs one answer, and the reassuring half of a mixed change is the wrong
    /// one to give them.
    pub fn cost(&self) -> Option<Cost> {
        self.changes.iter().map(|c| c.cost).max()
    }

    /// Fields that cannot be applied without restarting the process, so the UI
    /// can say so instead of reporting a success the bridge did not deliver.
    ///
    /// **`NewSession` counts too**, and it did not until 2026-08-05. The filter
    /// was `ProcessRestart` alone, so a change to the broker or the Sparkplug
    /// identity — which [#49] means this process cannot carry out either — was
    /// missing from the one list a form renders under *"waiting for a restart"*.
    /// The headline said "NOT in force" while the list beneath it was silently
    /// empty, and a test that searched the page for `broker_host` passed on the
    /// change table instead.
    ///
    /// If #49 is ever closed — a new session opened without a process restart —
    /// this filter narrows again, and the test named for the broker is what will
    /// say so.
    pub fn needs_restart(&self) -> Vec<&'static str> {
        self.changes
            .iter()
            .filter(|c| matches!(c.cost, Cost::ProcessRestart | Cost::NewSession))
            .map(|c| c.field)
            .collect()
    }
}

/// Compare two configurations and say what applying the second one costs.
///
/// Pure: no clock, no socket, no task. Everything about *when* and *how* the
/// plan is carried out belongs to the supervisor; this decides only *what*.
///
/// `served` is the set of meters the runtime has a poll task for — the truth
/// from [`Heartbeats::meters`](crate::app::poll_publish::Heartbeats::meters),
/// not something derivable from either configuration. See [`classify_meters`].
pub fn classify(old: &BridgeConfig, new: &BridgeConfig, served: &[crate::domain::MeterId]) -> Plan {
    // EXHAUSTIVE, deliberately — no `..`. A new field on `BridgeConfig` breaks
    // this line, and breaking it is the point: see the module docs.
    let BridgeConfig {
        api_base: old_api_base,
        credentials: old_credentials,
        http_timeout: old_http_timeout,
        meters: old_meters,
        group_id: old_group_id,
        node_id: old_node_id,
        broker_host: old_broker_host,
        broker_port: old_broker_port,
        bd_seq_path: old_bd_seq_path,
        poll: old_poll,
        policy: old_policy,
        log_dir: old_log_dir,
        log_keep: old_log_keep,
        ui_port: old_ui_port,
    } = old;
    let BridgeConfig {
        api_base: new_api_base,
        credentials: new_credentials,
        http_timeout: new_http_timeout,
        meters: new_meters,
        group_id: new_group_id,
        node_id: new_node_id,
        broker_host: new_broker_host,
        broker_port: new_broker_port,
        bd_seq_path: new_bd_seq_path,
        poll: new_poll,
        policy: new_policy,
        log_dir: new_log_dir,
        log_keep: new_log_keep,
        ui_port: new_ui_port,
    } = new;

    let mut plan = Plan::default();
    let mut note = |field, cost| plan.changes.push(Change { field, cost });

    // PROCESS RESTART — and these three were classified `Hot` until a review
    // caught it on 2026-08-04.
    //
    // `Cost::Hot` says "in force immediately". They were not: the smart-me
    // client and the source that holds them are built ONCE, before the poll task
    // is spawned (`supervisor.rs`, `SmartMeClient::new` / `SmartMeCloudSource::new`),
    // and nothing rebuilds either. The poll loop reads exactly two things from
    // the handle — the period and the staleness policy — so an `api_base` stored
    // by `apply` changed what `current()` REPORTED while every request still went
    // to the old endpoint. That is the precise lie this project exists to
    // prevent, committed in the module whose job is to prevent it.
    //
    // Reclassified rather than implemented, deliberately: making them genuinely
    // hot means rebuilding the source inside the poll task, which owns a generic
    // `S: Source` chosen so tests can inject a fake. That is a design change and
    // it is [#52], not a patch.
    if old_api_base != new_api_base {
        note("api_base", Cost::ProcessRestart);
    }
    if old_http_timeout != new_http_timeout {
        note("http_timeout", Cost::ProcessRestart);
    }
    if old_poll != new_poll {
        note("publish_period_secs", Cost::Hot);
    }
    if old_policy != new_policy {
        note("staleness policy", Cost::Hot);
    }

    // NEW SESSION — the topic namespace and the will are fixed at CONNECT.
    //
    // The group and node are not merely "where we publish": they are baked into
    // the will registered in the CONNECT packet, so changing them without
    // reconnecting would leave the broker holding a death certificate addressed
    // to a node that no longer exists under that name.
    if old_group_id != new_group_id {
        note("group_id", Cost::NewSession);
    }
    if old_node_id != new_node_id {
        note("node_id", Cost::NewSession);
    }
    if old_broker_host != new_broker_host {
        note("broker_host", Cost::NewSession);
    }
    if old_broker_port != new_broker_port {
        note("broker_port", Cost::NewSession);
    }
    // The session number is READ at connect and written across restarts. Moving
    // the file mid-session would make the next `bdSeq` come from somewhere the
    // current session never saw.
    if old_bd_seq_path != new_bd_seq_path {
        note("state directory", Cost::NewSession);
    }
    // PROCESS RESTART — see the module docs. Both of these are honest
    // admissions, not oversights.
    if old_log_dir != new_log_dir {
        note("log_dir", Cost::ProcessRestart);
    }
    if old_log_keep != new_log_keep {
        note("log_keep", Cost::ProcessRestart);
    }
    // Not in the file at all, so a reload cannot carry it. It is compared anyway
    // rather than skipped, because a `BridgeConfig` can be built by hand and a
    // silent difference here would be the one nobody notices.
    if old_credentials != new_credentials {
        note("smart-me credential", Cost::ProcessRestart);
    }
    // PROCESS RESTART, and it read `NewSession` until a review caught it on
    // 2026-08-04.
    //
    // The costs here are named for what the OPERATOR sees, and `NewSession`
    // means one specific thing on the wire: NDEATH, a new `bdSeq`, NBIRTH.
    // Moving the UI's port produces none of those — the Sparkplug session is not
    // touched at all. It produces exactly `ProcessRestart`'s description:
    // nothing until the container is restarted, because the listener is spawned
    // once from `main.rs` and nothing re-reads this.
    //
    // Worth keeping as a lesson: the compiler DID force this field to be
    // classified — the exhaustive destructure above would not build otherwise —
    // and the classification was then got wrong, which no compiler can catch.
    // The guarantee is that a question is asked, never that it is answered well.
    //
    // Two things followed from the wrong answer: `apply` warned "the Sparkplug
    // identity or the broker changed" when neither had, and `needs_restart()`
    // filters on `ProcessRestart`, so a form would not have listed `ui_port`
    // among the fields waiting for a restart — which is precisely what it is.
    if old_ui_port != new_ui_port {
        note("ui_port", Cost::ProcessRestart);
    }

    classify_meters(old_meters, new_meters, served, &mut plan);
    plan
}

/// The meter set, which is the only part where the cost depends on *which* way
/// the field moved.
/// What a change to the meter set costs, in the terms the RUNTIME can honour.
///
/// # Only a SERVED meter's on/off switch is a certificate
///
/// This function used to answer as though the meter set were live. It is not.
/// `supervisor::run_with_control` reads it once, at startup, spawns a poll task
/// per enabled meter and hands their serials to the driver. Nothing re-reads the
/// set. So of everything an operator can do to a meter, exactly one thing takes
/// effect on a running bridge: enabling or disabling a meter that **already has
/// a task**.
///
/// # `served` is passed in, because neither configuration can answer it
///
/// It was inferred here until 2026-08-08 — `old.iter().find(|m| m.enabled)`,
/// falling back to `old.first()` — and that inference was a faithful model of a
/// runtime that served ONE meter. Story 3.1 served them all on 2026-08-06 and
/// this function was not told, which made it wrong in the direction that costs
/// most: with four meters enabled, disabling the second, third or fourth was
/// classified `ProcessRestart`, so **no DDEATH was sent and the screen said a
/// restart would settle it**. A Sparkplug host went on showing a meter the
/// operator had switched off, at its last value, as current — the withheld
/// verdict ADR 0027 exists to forbid, arrived at from the configuration screen
/// instead of from a failed poll.
///
/// The set now comes from `Heartbeats::meters`, one entry per spawned task. A
/// configuration cannot supply it: `old` says what is *desired* and is rewritten
/// by every `apply`, so a meter enabled ten minutes ago appears there as enabled
/// while nothing polls it. Asking the tasks is the only way to be right, and it
/// is also the seam story 3.1 named as missing — *"nothing asserts that
/// `supervisor` spawns one task per meter; the heartbeat count is the seam that
/// would prove it"*. It is load-bearing now.
///
/// Three lies followed from getting this wrong, all found by a fresh-context
/// review on 2026-08-05 and all reported to the operator as success:
///
/// - **Renaming a meter** read as "one meter gone, another arrived" — two
///   certificates — and `apply`'s `device_id`-preservation lookup keys on the
///   name, so on a rename it found nothing and stored the NEW `device_id` while
///   the poll loop kept fetching the old one. That is [#52] reachable through
///   the front door.
/// - **Correcting a serial** un-declared the old device and declared the new
///   one, after which every reading — which carries the serial the smart-me API
///   reports, not the configured one — was dropped as `DroppedUndeclaredDevice`.
///   Total silence until a restart, announced as *"one device re-announced"*.
/// - **Moving `enabled` from one meter to another** passed validation (still one
///   enabled), birthed the new device and buried the old, and then published
///   nothing at all, because the poll task was still bound to the first.
///
/// The honest classification is therefore narrow, and narrowness is the point: a
/// cost table exists to let an operator act on it.
fn classify_meters(
    old: &[MeterConfig],
    new: &[MeterConfig],
    served: &[crate::domain::MeterId],
    plan: &mut Plan,
) {
    // Whether the runtime has a poll task for this meter.
    //
    // Disabling a meter does NOT unbind its task — it only sends a DDEATH — so a
    // meter stays served across a disable, and re-enabling it is a birth rather
    // than a restart. That is the cycle `chaos_device_certificates` exercises
    // against a real broker, and it is why this asks the task set rather than
    // asking whether the meter is currently enabled.
    let is_served = |meter: &crate::domain::MeterId| served.contains(meter);

    fn find<'a>(set: &'a [MeterConfig], id: &crate::domain::MeterId) -> Option<&'a MeterConfig> {
        set.iter().find(|m| &m.meter == id)
    }
    fn restart(plan: &mut Plan, field: &'static str) {
        plan.changes.push(Change {
            field,
            cost: Cost::ProcessRestart,
        });
    }

    // Anything about a meter that is not the served one's on/off switch.
    for before in old {
        match find(new, &before.meter) {
            Some(after) => {
                // EXHAUSTIVE, deliberately — no `..`. A new field on
                // `MeterConfig` breaks this line, and breaking it is the point:
                // the guarantee `classify` gives for `BridgeConfig` stopped at
                // the outer struct until 2026-08-05, so `meter_id` had no row in
                // the table at all.
                let MeterConfig {
                    meter: _,
                    device_id,
                    serial,
                    enabled,
                } = after;
                if *device_id != before.device_id {
                    restart(plan, "meters (device_id)");
                }
                if *serial != before.serial {
                    // NOT a device certificate: the DDATA serial comes from the
                    // smart-me response, never from the file, so re-declaring
                    // under a new one silences the meter until a restart.
                    restart(plan, "meters (serial)");
                }
                if *enabled != before.enabled {
                    if is_served(&before.meter) {
                        // The one genuinely live change, and it is now live for
                        // EVERY meter with a task rather than for one of them.
                        // Disabling buries the device; re-enabling births it
                        // again, and the poll task is still bound to it either
                        // way.
                        if before.enabled {
                            plan.deaths.push(before.serial.clone());
                        } else {
                            plan.births.push(before.serial.clone());
                        }
                        plan.changes.push(Change {
                            field: "meters (enabled)",
                            cost: Cost::DeviceCertificate,
                        });
                    } else {
                        // A meter with no task. Enabling it declares a device
                        // nothing polls; refusing to call that a certificate is
                        // what stops the screen reporting a silence as a
                        // success.
                        restart(plan, "meters (enabled)");
                    }
                }
            }
            // Removed from the configuration entirely. If it was the served
            // meter, the device must be buried; either way nothing takes its
            // place without a restart.
            None => {
                // A death is owed only if that DEVICE stops existing. A rename
                // arrives here as removed-plus-added while the serial — which is
                // the device level of the topic — is unchanged, so burying it
                // would tell a host that a device it can still see has gone.
                let device_survives = new.iter().any(|m| m.serial == before.serial && m.enabled);
                // And a device is only owed a burial if it was ever BORN, which
                // means the runtime had a task for it. A meter added and removed
                // between two restarts never reached the wire, so a DDEATH for
                // it would bury something no host has heard of.
                if before.enabled && is_served(&before.meter) && !device_survives {
                    plan.deaths.push(before.serial.clone());
                    plan.changes.push(Change {
                        field: "meters (removed)",
                        cost: Cost::DeviceCertificate,
                    });
                }
                restart(plan, "meters (removed)");
            }
        }
    }

    for after in new {
        if find(old, &after.meter).is_none() {
            // Same reasoning in the other direction: a renamed meter is not a
            // new device, so nothing is born for it.
            // A meter the process has never seen. Nothing polls it until the
            // runtime is rebuilt, so no birth is claimed here: a DBIRTH for a
            // device that will never carry a reading is the silence-reported-as-
            // success case in its purest form.
            restart(plan, "meters (added)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state_machine::Policy;
    use crate::domain::MeterId;
    use std::time::Duration;

    fn meter(id: &str, serial: &str, enabled: bool) -> MeterConfig {
        MeterConfig {
            meter: MeterId::new(id),
            device_id: format!("device-{id}"),
            serial: Serial::new(serial),
            enabled,
        }
    }

    /// The meters the runtime has a task for.
    ///
    /// Every case below `base()` builds models a bridge that started with
    /// `garage` enabled, which is what these tests always meant — it was
    /// inferred from the configuration until 2026-08-08 and is now stated.
    fn served() -> Vec<MeterId> {
        vec![MeterId::new("garage")]
    }

    fn base() -> BridgeConfig {
        BridgeConfig {
            api_base: "https://api.smart-me.com".to_string(),
            credentials: smart_me_client::Credentials::ClientCredentials {
                client_id: "id".to_string(),
                client_secret: "secret".to_string(),
            },
            http_timeout: Duration::from_secs(10),
            meters: vec![meter("garage", "9202685", true)],
            group_id: "Group".to_string(),
            node_id: "Node".to_string(),
            broker_host: "broker".to_string(),
            broker_port: 1883,
            bd_seq_path: std::path::PathBuf::from("/data/bdseq.toml"),
            poll: crate::app::PollConfig {
                interval: Duration::from_secs(30),
                fetch_timeout: Duration::from_secs(10),
            },
            policy: Policy::DEFAULT,
            log_dir: None,
            log_keep: None,
            ui_port: None,
        }
    }

    #[test]
    fn an_unchanged_configuration_costs_nothing() {
        let plan = classify(&base(), &base(), &served());
        assert!(plan.is_empty());
        assert_eq!(plan.cost(), None);
        assert!(plan.births.is_empty() && plan.deaths.is_empty());
    }

    #[test]
    fn the_publish_period_is_hot() {
        let mut new = base();
        new.poll.interval = Duration::from_secs(5);
        let plan = classify(&base(), &new, &served());
        assert_eq!(plan.cost(), Some(Cost::Hot));
        assert!(
            plan.births.is_empty() && plan.deaths.is_empty(),
            "changing the period must not disturb a single device: {plan:?}"
        );
    }

    #[test]
    fn the_identity_and_the_broker_cost_a_new_session() {
        for mutate in [
            (|c: &mut BridgeConfig| c.group_id = "Other".into()) as fn(&mut BridgeConfig),
            |c: &mut BridgeConfig| c.node_id = "Other".into(),
            |c: &mut BridgeConfig| c.broker_host = "elsewhere".into(),
            |c: &mut BridgeConfig| c.broker_port = 8883,
        ] {
            let mut new = base();
            mutate(&mut new);
            let plan = classify(&base(), &new, &served());
            assert_eq!(
                plan.cost(),
                Some(Cost::NewSession),
                "the will is registered at CONNECT, so this cannot be hot: {plan:?}"
            );
        }
    }

    /// **Story 5.2 AC4 at fleet scale — the defect story 3.1 left behind.**
    ///
    /// Until 2026-08-08 the served meter was inferred as *"the first enabled one
    /// in `old`"*, which was a faithful model of a runtime that served one meter
    /// and became wrong on 2026-08-06 when story 3.1 served them all. With four
    /// meters running, disabling any but the first was classified
    /// `ProcessRestart`: no DDEATH was sent, and the screen told the operator a
    /// restart would settle it. A Sparkplug host went on showing a meter that had
    /// been switched off, at its last value, as current — ADR 0027's withheld
    /// verdict, reached from the configuration screen instead of a failed poll.
    ///
    /// FALSIFIED 2026-08-08 by restoring the inference
    /// (`let served = old.iter().find(|m| m.enabled).or_else(|| old.first());`
    /// and `Some(s) if s.meter == before.meter`). Copied from the run:
    ///
    /// ```text
    /// test app::reconfigure::tests::every_served_meters_switch_is_a_certificate_not_only_the_firsts ... FAILED
    /// test app::reconfigure::tests::re_enabling_a_meter_that_kept_its_task_is_a_birth ... FAILED
    ///
    /// thread '…every_served_meters_switch_is_a_certificate_not_only_the_firsts' (362) panicked at
    /// crates/smartme-bridge/src/app/reconfigure.rs:582:13:
    /// assertion `left == right` failed: disabling cellar must bury its device: a host
    /// that is not told goes on showing a switched-off meter's last value as current
    ///   left: []
    ///  right: [Serial("9202686")]
    ///
    /// test result: FAILED. 11 passed; 2 failed
    /// ```
    ///
    /// **`left: []`** — not a wrong serial, no certificate at all. It dies on
    /// `cellar`, the SECOND meter and the first iteration the inference gets
    /// wrong: the loop walks all four positions precisely because the old code
    /// was right about `garage` and silent about the other three. The eleven
    /// tests that stayed green under the same mutation are the measure of how
    /// long this could have lived: every one of them describes a one-meter
    /// bridge.
    #[test]
    fn every_served_meters_switch_is_a_certificate_not_only_the_firsts() {
        let fleet = vec![
            meter("garage", "9202685", true),
            meter("cellar", "9202686", true),
            meter("workshop", "9202687", true),
            meter("barn", "9202688", true),
        ];
        // All four have a task: this is what `run_with_control` spawns for the
        // configuration above, and what `Heartbeats::meters` reports.
        let all: Vec<MeterId> = fleet.iter().map(|m| m.meter.clone()).collect();

        for (index, target) in fleet.iter().enumerate() {
            let mut old = base();
            old.meters = fleet.clone();
            let mut new = old.clone();
            new.meters[index].enabled = false;

            let plan = classify(&old, &new, &all);

            assert_eq!(
                plan.deaths,
                vec![target.serial.clone()],
                "disabling {} must bury its device: a host that is not told goes on \
                 showing a switched-off meter's last value as current",
                target.meter
            );
            assert_eq!(
                plan.cost(),
                Some(Cost::DeviceCertificate),
                "and it must not be reported as needing a restart, which is what \
                 the operator would have been told for {}",
                target.meter
            );
            assert!(
                plan.births.is_empty(),
                "nothing is born by switching one off: {plan:?}"
            );
            // The other three keep their tasks and their devices. A classifier
            // that buried the fleet on any change would satisfy the assertion
            // above and be catastrophic.
            assert_eq!(
                plan.deaths.len(),
                1,
                "exactly one device is buried; the siblings are untouched: {plan:?}"
            );
        }
    }

    /// The other direction, and the reason `served` is not simply "enabled".
    ///
    /// A meter that was switched off keeps its poll task — disabling sends a
    /// DDEATH, it does not unbind anything — so switching it back on is a birth
    /// rather than a restart. Asking `old` whether the meter is enabled would get
    /// this exactly backwards, which is why the set comes from the tasks.
    #[test]
    fn re_enabling_a_meter_that_kept_its_task_is_a_birth() {
        let fleet = vec![
            meter("garage", "9202685", true),
            meter("cellar", "9202686", false), // switched off earlier, task alive
        ];
        let all: Vec<MeterId> = fleet.iter().map(|m| m.meter.clone()).collect();

        let mut old = base();
        old.meters = fleet.clone();
        let mut new = old.clone();
        new.meters[1].enabled = true;

        let plan = classify(&old, &new, &all);
        assert_eq!(
            plan.births,
            vec![Serial::new("9202686")],
            "a meter with a task that is switched back on is born again, not \
             deferred to a restart: {plan:?}"
        );
        assert_eq!(plan.cost(), Some(Cost::DeviceCertificate));
    }

    /// And the guard that keeps the fix from over-reaching: a meter with NO task
    /// still needs a restart, however enabled the file says it is.
    ///
    /// This is the case `moving_the_enabled_flag_to_another_meter_needs_a_restart`
    /// covers from the other side, kept separate because the fix above could have
    /// been written as "every enabled meter is a certificate" — which would pass
    /// that test's sibling assertions and declare a device nothing polls.
    #[test]
    fn a_meter_the_runtime_never_started_is_a_restart_even_when_enabled() {
        let mut old = base();
        old.meters = vec![
            meter("garage", "9202685", true),
            meter("added-later", "9202699", false),
        ];
        let mut new = old.clone();
        new.meters[1].enabled = true;

        // Only `garage` has a task: `added-later` was written into the file after
        // the process started, so nothing polls it.
        let plan = classify(&old, &new, &served());
        assert!(
            plan.needs_restart().contains(&"meters (enabled)"),
            "a device nothing polls must not be declared: {plan:?}"
        );
        assert!(
            plan.births.is_empty(),
            "a DBIRTH for a device that will never carry a reading is the \
             silence-reported-as-success case in its purest form: {plan:?}"
        );
    }

    /// The heart of AC4: enabling births, disabling buries, and neither touches
    /// the session.
    #[test]
    fn enabling_a_meter_births_it_and_disabling_it_buries_it() {
        let mut off = base();
        off.meters[0].enabled = false;

        let on = classify(&off, &base(), &served());
        assert_eq!(on.cost(), Some(Cost::DeviceCertificate));
        assert_eq!(on.births, vec![Serial::new("9202685")]);
        assert!(on.deaths.is_empty());

        let gone = classify(&base(), &off, &served());
        assert_eq!(gone.cost(), Some(Cost::DeviceCertificate));
        assert_eq!(gone.deaths, vec![Serial::new("9202685")]);
        assert!(gone.births.is_empty());
    }

    /// **Moving `enabled` from one meter to another is NOT a certificate**, and
    /// calling it one reported a total silence as a success.
    ///
    /// The runtime binds the first enabled meter once, at startup. Enabling a
    /// different one declares a device nothing polls and buries the only one that
    /// was working: `validate` accepts it (still one enabled), the wire gets a
    /// DDEATH and a DBIRTH, and from the next tick every reading is dropped as
    /// `DroppedUndeclaredDevice`. Found by a fresh-context review, 2026-08-05.
    #[test]
    fn moving_the_enabled_flag_to_another_meter_needs_a_restart() {
        let mut old = base();
        old.meters = vec![
            meter("garage", "9202685", true),
            meter("cellar", "9202686", false),
        ];
        let mut new = old.clone();
        new.meters[0].enabled = false;
        new.meters[1].enabled = true;

        let plan = classify(&old, &new, &served());
        assert!(
            plan.needs_restart().contains(&"meters (enabled)"),
            "enabling a meter the poll task is not bound to must be reported as \
             needing a restart, or the operator is told a silence is a success: \
             {plan:?}"
        );
        assert!(
            !plan.births.contains(&Serial::new("9202686")),
            "a DBIRTH for a device that will never carry a reading is the \
             silence-reported-as-success case in its purest form: {plan:?}"
        );
        assert_eq!(plan.cost(), Some(Cost::ProcessRestart));
    }

    /// **Renaming a meter needs a restart**, and it had no row in the table at
    /// all until `classify_meters` destructured `MeterConfig`.
    ///
    /// A rename used to read as "one meter gone, another arrived" — two
    /// certificates — and `Control::apply`'s `device_id`-preservation lookup keys
    /// on the name, so on a rename it found nothing and stored the NEW
    /// `device_id` while the poll loop kept fetching the old one. [#52] through
    /// the front door.
    #[test]
    fn renaming_a_meter_needs_a_restart_and_claims_no_certificate() {
        let mut new = base();
        new.meters[0].meter = MeterId::new("garage-1");
        let plan = classify(&base(), &new, &served());
        assert_eq!(plan.cost(), Some(Cost::ProcessRestart));
        assert!(
            plan.births.is_empty() && plan.deaths.is_empty(),
            "a label change alters nothing on the wire: {plan:?}"
        );
    }

    /// A meter is identified by its id, not its index. Without this, sorting the
    /// list in a form would read as every device dying and being reborn.
    #[test]
    fn reordering_the_meter_list_is_not_a_change() {
        let mut old = base();
        old.meters = vec![
            meter("garage", "9202685", true),
            meter("cellar", "9202686", false),
        ];
        let mut new = old.clone();
        new.meters.reverse();
        let plan = classify(&old, &new, &served());
        assert!(
            plan.is_empty(),
            "reordering is not a change to any device: {plan:?}"
        );
    }

    /// The serial IS the device's topic level, so replacing it replaces the
    /// device — and the order matters, because the old topic must stop being
    /// current before the new one starts.
    #[test]
    fn changing_a_serial_needs_a_restart_rather_than_a_certificate() {
        let mut new = base();
        new.meters[0].serial = Serial::new("9202699");
        let plan = classify(&base(), &new, &served());
        assert!(
            plan.needs_restart().contains(&"meters (serial)"),
            "the DDATA serial comes from the smart-me RESPONSE, never from the \
             file, so re-declaring under a new one drops every reading as \
             DroppedUndeclaredDevice — total silence until a restart, and it was \
             announced as 'one device re-announced': {plan:?}"
        );
        assert!(
            plan.births.is_empty() && plan.deaths.is_empty(),
            "burying the working device and birthing one nothing can feed is \
             worse than doing nothing: {plan:?}"
        );
    }

    /// A mixed change must report the WORST cost. Reporting "hot" because one
    /// part of it is would be a half-truth an operator acts on.
    #[test]
    fn a_mixed_change_reports_the_most_disruptive_cost() {
        let mut new = base();
        new.poll.interval = Duration::from_secs(5); // hot
        new.broker_host = "elsewhere".into(); // new session
        let plan = classify(&base(), &new, &served());
        assert_eq!(plan.cost(), Some(Cost::NewSession));
        assert_eq!(plan.changes.len(), 2, "both must be listed: {plan:?}");
    }

    /// The category that exists to stop a form from lying.
    ///
    /// `ui_port` is in here because a review corrected it from `NewSession` on
    /// 2026-08-04: moving a listener produces no NDEATH, no new `bdSeq` and no
    /// NBIRTH, so calling it a new session sent an operator to look at their
    /// broker — and kept it out of `needs_restart()`, which is the one list a
    /// form shows.
    #[test]
    fn the_log_settings_the_credential_and_the_ui_port_need_a_restart() {
        let mut new = base();
        new.log_dir = Some("/data/logs".into());
        new.log_keep = Some(30);
        new.ui_port = Some(9090);
        let plan = classify(&base(), &new, &served());
        assert_eq!(plan.cost(), Some(Cost::ProcessRestart));
        assert_eq!(
            plan.needs_restart(),
            vec!["log_dir", "log_keep", "ui_port"],
            "every field a form must mark as waiting has to be in this list"
        );
        assert!(
            !plan.changes.iter().any(|c| c.cost == Cost::NewSession),
            "moving a listener does not disturb the Sparkplug session: {plan:?}"
        );
    }
}
