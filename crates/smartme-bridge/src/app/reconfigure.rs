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
    pub fn needs_restart(&self) -> Vec<&'static str> {
        self.changes
            .iter()
            .filter(|c| c.cost == Cost::ProcessRestart)
            .map(|c| c.field)
            .collect()
    }
}

/// Compare two configurations and say what applying the second one costs.
///
/// Pure: no clock, no socket, no task. Everything about *when* and *how* the
/// plan is carried out belongs to the supervisor; this decides only *what*.
pub fn classify(old: &BridgeConfig, new: &BridgeConfig) -> Plan {
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

    classify_meters(old_meters, new_meters, &mut plan);
    plan
}

/// The meter set, which is the only part where the cost depends on *which* way
/// the field moved.
fn classify_meters(old: &[MeterConfig], new: &[MeterConfig], plan: &mut Plan) {
    let served = |meters: &[MeterConfig]| -> Vec<MeterConfig> {
        meters.iter().filter(|m| m.enabled).cloned().collect()
    };
    let (was, now) = (served(old), served(new));

    // A meter is identified by its METER ID, not its position: reordering the
    // list in a form must not read as "one device died and another was born".
    let find = |set: &[MeterConfig], id: &crate::domain::MeterId| {
        set.iter().find(|m| &m.meter == id).cloned()
    };

    for gone in &was {
        match find(&now, &gone.meter) {
            None => {
                // Disabled or deleted. Either way the device stops existing for
                // the host, and saying so is what keeps the last value from
                // being read as current forever.
                plan.deaths.push(gone.serial.clone());
                plan.changes.push(Change {
                    field: "meters (disabled)",
                    cost: Cost::DeviceCertificate,
                });
            }
            Some(still) if still.serial != gone.serial => {
                // The serial IS the device's topic level, so changing it is not
                // an edit to a device — it is one device replaced by another.
                // Both certificates, and the death first: the old topic must
                // stop being current before the new one starts.
                plan.deaths.push(gone.serial.clone());
                plan.births.push(still.serial.clone());
                plan.changes.push(Change {
                    field: "meters (serial)",
                    cost: Cost::DeviceCertificate,
                });
            }
            Some(still) if still.device_id != gone.device_id => {
                // Which smart-me device is polled. No Sparkplug certificate is
                // owed — but this is NOT hot either, and said so until a review
                // on 2026-08-04: `device_id` is moved into `SmartMeCloudSource`
                // before the poll task is spawned and nothing rebuilds it, so a
                // change here left the bridge polling the previous device while
                // the model reported the new one. [#52].
                plan.changes.push(Change {
                    field: "meters (device_id)",
                    cost: Cost::ProcessRestart,
                });
            }
            Some(_) => {}
        }
    }

    for arrived in &now {
        if find(&was, &arrived.meter).is_none() {
            plan.births.push(arrived.serial.clone());
            plan.changes.push(Change {
                field: "meters (enabled)",
                cost: Cost::DeviceCertificate,
            });
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
            policy: Policy { max_age_ms: 90_000 },
            log_dir: None,
            log_keep: None,
            ui_port: None,
        }
    }

    #[test]
    fn an_unchanged_configuration_costs_nothing() {
        let plan = classify(&base(), &base());
        assert!(plan.is_empty());
        assert_eq!(plan.cost(), None);
        assert!(plan.births.is_empty() && plan.deaths.is_empty());
    }

    #[test]
    fn the_publish_period_is_hot() {
        let mut new = base();
        new.poll.interval = Duration::from_secs(5);
        let plan = classify(&base(), &new);
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
            let plan = classify(&base(), &new);
            assert_eq!(
                plan.cost(),
                Some(Cost::NewSession),
                "the will is registered at CONNECT, so this cannot be hot: {plan:?}"
            );
        }
    }

    /// The heart of AC4: enabling births, disabling buries, and neither touches
    /// the session.
    #[test]
    fn enabling_a_meter_births_it_and_disabling_it_buries_it() {
        let mut off = base();
        off.meters[0].enabled = false;

        let on = classify(&off, &base());
        assert_eq!(on.cost(), Some(Cost::DeviceCertificate));
        assert_eq!(on.births, vec![Serial::new("9202685")]);
        assert!(on.deaths.is_empty());

        let gone = classify(&base(), &off);
        assert_eq!(gone.cost(), Some(Cost::DeviceCertificate));
        assert_eq!(gone.deaths, vec![Serial::new("9202685")]);
        assert!(gone.births.is_empty());
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
        let plan = classify(&old, &new);
        assert!(
            plan.is_empty(),
            "reordering is not a change to any device: {plan:?}"
        );
    }

    /// The serial IS the device's topic level, so replacing it replaces the
    /// device — and the order matters, because the old topic must stop being
    /// current before the new one starts.
    #[test]
    fn changing_a_serial_buries_the_old_device_and_births_the_new_one() {
        let mut new = base();
        new.meters[0].serial = Serial::new("9202699");
        let plan = classify(&base(), &new);
        assert_eq!(plan.deaths, vec![Serial::new("9202685")]);
        assert_eq!(plan.births, vec![Serial::new("9202699")]);
    }

    /// A mixed change must report the WORST cost. Reporting "hot" because one
    /// part of it is would be a half-truth an operator acts on.
    #[test]
    fn a_mixed_change_reports_the_most_disruptive_cost() {
        let mut new = base();
        new.poll.interval = Duration::from_secs(5); // hot
        new.broker_host = "elsewhere".into(); // new session
        let plan = classify(&base(), &new);
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
        let plan = classify(&base(), &new);
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
