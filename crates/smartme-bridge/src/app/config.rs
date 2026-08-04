//! One configuration model, one validation, one way to obtain a valid one
//! (Story 5.1).
//!
//! # Why this module exists at all
//!
//! Until 2026-08-03 `main.rs` assembled [`BridgeConfig`] field by field out of
//! six `env::var` calls, applying what rules there were inline. That worked
//! while the binary was the only consumer. **FR46 gave it a second one** — the
//! configuration web UI — and two consumers assembling the same struct by hand
//! is how their rules drift apart: the first time a bound changes, one of them
//! keeps the old one, and the disagreement shows up as a value a browser accepts
//! and the bridge refuses to boot on.
//!
//! So the type is the enforcement. [`RawConfig`] holds what arrived, all of it
//! optional and none of it trusted; [`validate`] is the only way to turn one
//! into a [`BridgeConfig`]; and **a `BridgeConfig` that exists is a
//! `BridgeConfig` that was validated.**
//!
//! # Every fault, not the first
//!
//! [`validate`] returns a *collection* of [`Fault`]s. The obvious `?`-on-first-
//! error shape is the defect: with six required values, a first run became up to
//! six edit-restart cycles, each revealing exactly one more thing.
//!
//! # Secrets never appear in a fault
//!
//! A fault carries the **field name** and what is wrong with it, never the
//! value. That matters for one field in particular — the smart-me client secret
//! — and it is asserted by a test rather than left to care, because
//! [ADR 0019](../../../docs/adr/0019-no-auth-on-the-config-ui-secrets-are-write-only.md)
//! makes *never rendered, never traced* a property of the product and not a
//! habit of whoever writes the next error message.

use std::path::PathBuf;
use std::time::Duration;

use smart_me_client::{Credentials, SmartMeClient};

use crate::app::poll_publish::PollConfig;
use crate::app::supervisor::BridgeConfig;
use crate::core::state_machine::Policy;
use crate::domain::{MeterId, Serial};

/// Shortest publish period an operator may set.
///
/// [ADR 0020](../../../docs/adr/0020-the-publish-period-is-bounded-and-cannot-be-turned-off.md),
/// ratified by Guy on 2026-08-03. Below this the bridge becomes a load generator
/// against the smart-me cloud for no gain — the meters do not update that fast.
pub const PERIOD_MIN: Duration = Duration::from_secs(5);

/// Longest publish period an operator may set, and **the reason there is a
/// maximum at all**.
///
/// [ADR 0018](../../../docs/adr/0018-no-primary-host-state-the-repair-is-host-initiated.md)
/// ruled out Primary Host / STATE on the grounds that recovery is
/// host-initiated, and step 1 of that loop is *the bridge publishes DDATA every
/// poll*. The period is therefore the worst-case delay before a host that
/// restarted can notice a node whose BIRTH it never saw and ask for a rebirth.
/// A period of "never" would not slow that repair down — it would remove it.
pub const PERIOD_MAX: Duration = Duration::from_secs(300);

/// The period when none is configured. **Exactly what was hard-coded before
/// this setting existed**, so adopting the setting is not itself a change of
/// behaviour: a release that shipped both would be two changes wearing one name.
pub const PERIOD_DEFAULT: Duration = Duration::from_secs(30);

/// How many meters the *runtime* can serve today.
///
/// The **model** holds any number (Story 5.1 AC4), because the configuration
/// screen is built against the model and a form built against a singular field
/// would be built twice. The **runtime** still serves one. Enabling more than
/// this is refused rather than truncated — the fault is raised in [`validate`]
/// under the field name `enabled meters`.
pub const RUNTIME_METER_LIMIT: usize = 1;

/// A meter as configured: its identity, and whether it is published at all.
///
/// `enabled` is not decoration. Guy runs four meters and one of them is not
/// currently connected; *configured but not enabled* is how that is expressed
/// without deleting its settings and retyping them later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterConfig {
    /// The logical meter name used in the Sparkplug metric path.
    pub meter: MeterId,
    /// smart-me device id backing it.
    pub device_id: String,
    /// The device serial — the Sparkplug device identifier.
    pub serial: Serial,
    /// Whether this meter is published.
    pub enabled: bool,
}

/// Configuration exactly as it arrived, from an environment or a form. Every
/// field optional, no rule applied, nothing trusted.
#[derive(Clone, Default)]
pub struct RawConfig {
    pub api_base: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub group_id: Option<String>,
    pub node_id: Option<String>,
    pub broker_host: Option<String>,
    pub broker_port: Option<String>,
    pub state_dir: Option<String>,
    pub publish_period_secs: Option<String>,
    pub log_dir: Option<String>,
    pub log_keep: Option<usize>,
    pub ui_port: Option<u16>,
    pub meters: Vec<RawMeter>,
}

/// Hand-written so `{:?}` cannot leak the credential.
///
/// **Found by falsification, 2026-08-03.** A mutation made a test fail, and the
/// panic message printed `client_secret: Some("s3cr3t-do-not-print")` — from the
/// derived `Debug` on this struct. `StoredSecrets` had a hand-written one from
/// the start; the struct the secret actually ARRIVES through did not. Every
/// `tracing::debug!(?raw)` anyone adds later would have leaked it, and nothing
/// would have complained.
///
/// ADR 0019 is a property of the product, so it cannot rest on remembering not
/// to derive `Debug`. Where a secret lives, the derive is the defect.
impl std::fmt::Debug for RawConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawConfig")
            .field("api_base", &self.api_base)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .field("group_id", &self.group_id)
            .field("node_id", &self.node_id)
            .field("broker_host", &self.broker_host)
            .field("broker_port", &self.broker_port)
            .field("state_dir", &self.state_dir)
            .field("publish_period_secs", &self.publish_period_secs)
            .field("log_dir", &self.log_dir)
            .field("log_keep", &self.log_keep)
            .field("ui_port", &self.ui_port)
            .field("meters", &self.meters)
            .finish()
    }
}

/// One meter as it arrived.
#[derive(Debug, Clone, Default)]
pub struct RawMeter {
    pub meter_id: Option<String>,
    pub device_id: Option<String>,
    pub serial: Option<String>,
    /// Absent means enabled: a meter someone bothered to configure is one they
    /// meant to publish. Disabling is the deliberate act, so it is the one that
    /// has to be written down.
    pub enabled: Option<bool>,
}

/// One thing wrong with the configuration.
///
/// `field` is a name, never a value. See the module documentation: the client
/// secret must not reach a log, an error, or a screen, and the way to guarantee
/// that is for it never to enter the message in the first place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The setting at fault, named as the operator knows it.
    pub field: String,
    /// Where the operator changes it, where that can be named.
    ///
    /// **Both names are needed, and for different readers.** A form shows
    /// *Sparkplug group id*; someone fixing a refusal from a shell needs the key
    /// or the variable they actually type. Printing only the prose name is a
    /// regression this project caught the moment it happened —
    /// `startup_banner` asserts the failure names it, because a message that does
    /// not is one the reader has to translate before acting on.
    ///
    /// **Was `env_var: Option<&'static str>` until 2026-08-04.** [ADR 0023] moved
    /// every setting but the credential into `config.toml`, which would have left
    /// nine faults directing the operator to `SMARTME_*` variables that no longer
    /// do anything — the same misdirection the field exists to prevent, merely
    /// pointing somewhere new.
    ///
    /// [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
    pub source: Option<Source>,
    /// What is wrong, and where it matters, what it will cost.
    pub problem: String,
}

/// Where a setting is edited — and there are exactly two places, by decision
/// rather than by accident ([ADR 0023]).
///
/// [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A key in `config.toml`: changed in the web UI, or by hand for a headless
    /// bring-up. Owned rather than `&'static str` because a meter's key carries
    /// its index — `meters[2].serial` names one meter out of four, and "the
    /// serial is missing" across four of them does not.
    File(String),
    /// An environment variable. The credential only — nothing else is left.
    Env(&'static str),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // The file is named, not just the key. "group_id" alone sends a
            // reader looking for an environment variable of that name, which is
            // where this whole class of message went wrong the first time.
            Source::File(key) => write!(f, "config.toml: {key}"),
            Source::Env(var) => write!(f, "environment: {var}"),
        }
    }
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            Some(source) => write!(f, "{} ({source}): {}", self.field, self.problem),
            None => write!(f, "{}: {}", self.field, self.problem),
        }
    }
}

/// Everything wrong with the configuration, together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigErrors(pub Vec<Fault>);

impl std::fmt::Display for ConfigErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "the configuration was refused; {} problem(s) found, all of them listed \
             so this takes one pass and not one restart each:",
            self.0.len()
        )?;
        for fault in &self.0 {
            writeln!(f, "  - {fault}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigErrors {}

fn present(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

fn required<'a>(
    value: &'a Option<String>,
    field: &str,
    source: Option<Source>,
    consequence: &str,
    faults: &mut Vec<Fault>,
) -> Option<&'a str> {
    match present(value) {
        Some(v) => Some(v),
        None => {
            faults.push(Fault {
                field: field.to_string(),
                source,
                problem: format!("is missing or empty — {consequence}"),
            });
            None
        }
    }
}

/// The publish period, bounded per ADR 0020.
fn period(raw: &Option<String>, faults: &mut Vec<Fault>) -> Duration {
    let Some(text) = present(raw) else {
        return PERIOD_DEFAULT;
    };
    let Ok(secs) = text.parse::<u64>() else {
        faults.push(Fault {
            field: "publish period".to_string(),
            source: Some(Source::File("publish_period_secs".into())),
            problem: format!(
                "is not a whole number of seconds: {text:?}. There is no value meaning \
                 'off' — see below for why"
            ),
        });
        return PERIOD_DEFAULT;
    };
    let candidate = Duration::from_secs(secs);
    if candidate < PERIOD_MIN || candidate > PERIOD_MAX {
        faults.push(Fault {
            field: "publish period".to_string(),
            source: Some(Source::File("publish_period_secs".into())),
            problem: format!(
                "is {secs}s, outside {}s..={}s. The maximum is not a preference: the \
                 periodic publish is what lets a SCADA host that restarted notice a node \
                 whose BIRTH it never saw and ask for a rebirth (ADR 0018), so a period \
                 long enough — or an 'off' — would not slow that repair down, it would \
                 remove it",
                PERIOD_MIN.as_secs(),
                PERIOD_MAX.as_secs()
            ),
        });
        return PERIOD_DEFAULT;
    }
    candidate
}

/// A serial must be usable as a Sparkplug device identifier **and** must be the
/// serial the meter actually reports.
fn check_serial(
    raw: &str,
    index: usize,
    node: Option<&sparkplug_b::EdgeNode>,
    faults: &mut Vec<Fault>,
) {
    let field = format!("meter {index} serial");
    // A leading zero is the one that has actually happened here, and it does not
    // fail loudly: the bridge runs, the node births, the tags appear in the tag
    // browser, and every reading is discarded as DroppedUndeclaredDevice because
    // the serial the meter reports never matches the one declared. A healthy
    // bridge that publishes nothing is far more expensive than a refusal now.
    //
    // KNOW WHAT THIS RULE ACTUALLY IS. The real requirement is *the serial must
    // be the one smart-me reports*, which cannot be checked offline. The leading
    // zero is a proxy for it, generalised from a single incident — so it can in
    // principle refuse a legitimate serial, and there is deliberately no override.
    //
    // Guy confirmed on 2026-08-03 that none of his four meters carries one, and
    // chose the hard refusal over a warning, on the grounds that the failure it
    // prevents is silent and a startup WARN would drown — which is exactly what
    // #44 had just demonstrated about warnings nobody can see. If a meter with a
    // genuine leading zero ever appears, this is a code change and not a
    // configuration one, and that is the accepted cost rather than an oversight.
    if raw.len() > 1 && raw.starts_with('0') {
        faults.push(Fault {
            field: field.clone(),
            source: None,
            problem: format!(
                "has a leading zero ({} digits). smart-me reports it without one, so every \
                 reading would be discarded as DroppedUndeclaredDevice — the bridge would \
                 run, the node would appear in the tag browser, and no value would ever \
                 arrive",
                raw.len()
            ),
        });
    }
    // Reuse the topic grammar rather than restating it: a second copy of these
    // rules is a second place for them to drift.
    if let Some(node) = node {
        if let Err(error) = node.device_topic(sparkplug_b::MessageType::DBirth, raw) {
            faults.push(Fault {
                field,
                source: None,
                problem: format!(
                    "cannot be a Sparkplug topic level ({error}); the node would connect, \
                     never birth, and publish nothing"
                ),
            });
        }
    }
}

/// Turn untrusted input into a configuration, or into every reason it is not one.
///
/// This is the only way to obtain a [`BridgeConfig`]. See the module docs.
pub fn validate(raw: RawConfig) -> Result<BridgeConfig, ConfigErrors> {
    let mut faults = Vec::new();

    let client_id = required(
        &raw.client_id,
        "smart-me client id",
        Some(Source::Env("SMARTME_CLIENT_ID")),
        "the bridge cannot authenticate against the smart-me cloud",
        &mut faults,
    )
    .map(str::to_owned);
    // NOTE the asymmetry with every other field: nothing derived from the secret
    // is ever put into a fault. Not its length, not a prefix, not a mask.
    let client_secret = match present(&raw.client_secret) {
        Some(v) => Some(v.to_owned()),
        None => {
            faults.push(Fault {
                field: "smart-me client secret".to_string(),
                source: Some(Source::Env("SMARTME_CLIENT_SECRET")),
                problem: "is missing or empty".to_string(),
            });
            None
        }
    };

    let group_id = required(
        &raw.group_id,
        "Sparkplug group id",
        Some(Source::File("group_id".into())),
        "there is deliberately no default: a Sparkplug host PERSISTS what it discovers, so \
         publishing into the wrong namespace creates a tag folder that outlives the process \
         and has to be deleted by hand",
        &mut faults,
    )
    .map(str::to_owned);
    let node_id = required(
        &raw.node_id,
        "Sparkplug node id",
        Some(Source::File("node_id".into())),
        "same reason as the group: the namespace is not recoverable by restarting with \
         better settings",
        &mut faults,
    )
    .map(str::to_owned);
    let broker_host = required(
        &raw.broker_host,
        "broker host",
        Some(Source::File("broker_host".into())),
        "there is nowhere to publish",
        &mut faults,
    )
    .map(str::to_owned);

    let broker_port = match present(&raw.broker_port) {
        None => 1883,
        Some(text) => match text.parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                faults.push(Fault {
                    field: "broker port".to_string(),
                    source: Some(Source::File("broker_port".into())),
                    problem: format!(
                        "is not a port number: {text:?}. A typo must not silently connect \
                         somewhere else"
                    ),
                });
                1883
            }
        },
    };

    let interval = period(&raw.publish_period_secs, &mut faults);

    // Built early so the serial checks can reuse the real topic grammar. If the
    // identifiers are themselves bad they are already a fault, and the serials
    // are simply checked for the rules that do not depend on them.
    let node = match (&group_id, &node_id) {
        (Some(g), Some(n)) => match sparkplug_b::EdgeNode::new(g.clone(), n.clone()) {
            Ok(node) => Some(node),
            Err(error) => {
                faults.push(Fault {
                    field: "Sparkplug group id / node id".to_string(),
                    source: None,
                    problem: format!("cannot form a topic: {error}"),
                });
                None
            }
        },
        _ => None,
    };

    let mut meters: Vec<MeterConfig> = Vec::new();
    for (index, raw_meter) in raw.meters.iter().enumerate() {
        // Every meter can be named now, and by its index.
        //
        // Until 2026-08-04 only meter 0 had a name to give — the environment
        // carried exactly one meter, and the rest could arrive only through a UI
        // that did not exist. With the meters in the file (ADR 0023) there is a
        // key for each, and "the serial is missing" over four meters is not a
        // message anyone can act on.
        let key = |field: &str| Some(Source::File(format!("meters[{index}].{field}")));
        let meter_id = required(
            &raw_meter.meter_id,
            &format!("meter {index} id"),
            key("meter_id"),
            "it names the metric path a SCADA host will show",
            &mut faults,
        );
        let device_id = required(
            &raw_meter.device_id,
            &format!("meter {index} device id"),
            key("device_id"),
            "it is what the smart-me API is asked for",
            &mut faults,
        );
        let serial = required(
            &raw_meter.serial,
            &format!("meter {index} serial"),
            key("serial"),
            "it binds the reading to the device that produced it",
            &mut faults,
        );
        if let Some(serial) = serial {
            check_serial(serial, index, node.as_ref(), &mut faults);
        }
        if let (Some(meter_id), Some(device_id), Some(serial)) = (meter_id, device_id, serial) {
            meters.push(MeterConfig {
                meter: MeterId::new(meter_id),
                device_id: device_id.to_owned(),
                serial: Serial::new(serial),
                enabled: raw_meter.enabled.unwrap_or(true),
            });
        }
    }

    if raw.meters.is_empty() {
        faults.push(Fault {
            field: "meters".to_string(),
            source: None,
            problem: "none configured — the bridge would connect, birth, and have nothing \
                      to say"
                .to_string(),
        });
    }

    // Uniqueness. Two meters sharing a serial share a Sparkplug device topic, so
    // whichever publishes last wins and the other silently disappears — the
    // reading is not wrong, it is attributed to the wrong device.
    duplicates(meters.iter().map(|m| m.serial.as_str()))
        .into_iter()
        .for_each(|serial| {
            faults.push(Fault {
                field: "meter serials".to_string(),
                source: None,
                problem: format!(
                    "{serial:?} is used by more than one meter; they would share one \
                     Sparkplug device topic and overwrite each other"
                ),
            })
        });
    duplicates(meters.iter().map(|m| m.meter.as_str()))
        .into_iter()
        .for_each(|meter| {
            faults.push(Fault {
                field: "meter ids".to_string(),
                source: None,
                problem: format!("{meter:?} is used by more than one meter"),
            })
        });

    // The model holds any number; the runtime does not. Refusing beats serving a
    // subset: a bridge that quietly published one of four meters would look
    // healthy in every way a human checks — node online, tags present, values
    // fresh — while three meters were simply absent.
    let enabled = meters.iter().filter(|m| m.enabled).count();
    if enabled > RUNTIME_METER_LIMIT {
        faults.push(Fault {
            field: "enabled meters".to_string(),
            source: None,
            problem: format!(
                "{enabled} are enabled and the runtime serves {RUNTIME_METER_LIMIT}. The \
                 configuration model accepts more so the UI can be built against its final \
                 shape, but serving them is not implemented yet — refusing here beats \
                 publishing a subset that looks complete"
            ),
        });
    }
    if enabled == 0 && !meters.is_empty() {
        faults.push(Fault {
            field: "enabled meters".to_string(),
            source: None,
            problem: "every configured meter is disabled; the bridge would publish nothing"
                .to_string(),
        });
    }

    // The endpoint, checked HERE rather than left to `SmartMeClient::new` at
    // startup.
    //
    // It was left to it in the first draft of this module, and that quietly broke
    // AC2: an operator with a bad endpoint AND a missing group id was told about
    // the group id, fixed it, restarted, and only then learnt about the endpoint.
    // Two cycles — the exact shape this story exists to remove.
    //
    // The rule is not restated. `SmartMeClient::new` owns it (it is the type that
    // refuses a non-TLS base), so it is asked, the same way the serial check asks
    // the topic grammar instead of reimplementing it. The client built here is
    // discarded; what is wanted is its verdict.
    let api_base = present(&raw.api_base)
        .unwrap_or(SmartMeClient::DEFAULT_BASE)
        .to_string();
    if let (Some(id), Some(secret)) = (&client_id, &client_secret) {
        if let Err(error) = SmartMeClient::new(
            api_base.clone(),
            Credentials::ClientCredentials {
                client_id: id.clone(),
                client_secret: secret.clone(),
            },
            Duration::from_secs(10),
        ) {
            faults.push(Fault {
                field: "smart-me API base".to_string(),
                source: Some(Source::File("api_base".into())),
                // `SmartMeError`'s Display is asserted elsewhere never to embed
                // credentials, which is why it is safe to interpolate here.
                problem: format!("was refused: {error}"),
            });
        }
    }

    if !faults.is_empty() {
        return Err(ConfigErrors(faults));
    }

    Ok(BridgeConfig {
        api_base,
        credentials: Credentials::ClientCredentials {
            client_id: client_id.expect("no faults means every required field is present"),
            client_secret: client_secret.expect("no faults means every required field is present"),
        },
        http_timeout: Duration::from_secs(10),
        meters,
        group_id: group_id.expect("no faults means every required field is present"),
        node_id: node_id.expect("no faults means every required field is present"),
        broker_host: broker_host.expect("no faults means every required field is present"),
        broker_port,
        bd_seq_path: PathBuf::from(present(&raw.state_dir).unwrap_or("/data")).join("bdseq.toml"),
        poll: PollConfig {
            interval,
            fetch_timeout: Duration::from_secs(10),
        },
        policy: Policy { max_age_ms: 90_000 },
        // Passed through unvalidated and unused by the runtime: `main.rs` has
        // already acted on them by the time this runs. They are here so a
        // reload can SEE them change — see `app::reconfigure`.
        log_dir: present(&raw.log_dir).map(str::to_owned),
        log_keep: raw.log_keep,
        ui_port: raw.ui_port,
    })
}

/// One line of what an operator is asked to confirm (Story 5.3 AC4, FR25).
///
/// **The serial and the topic are here because the mistake is invisible without
/// them.** `prd.md:135` asks for *"serial beside each so he can't cross-wire"*:
/// a confirmation screen showing only meter names would be a click that proves
/// nothing, since a name is exactly the part that looks right when it is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingRow {
    /// The meter as the operator named it.
    pub meter: String,
    /// The smart-me device actually polled — the half that is easy to
    /// cross-wire, because it is a UUID nobody recognises by sight.
    pub device_id: String,
    /// The serial, which becomes the device level of the topic.
    pub serial: String,
    /// The exact topic values will be published on.
    pub topic: String,
    /// Whether this meter is published at all.
    pub enabled: bool,
}

/// What will be published where, for a human to check before anything is.
///
/// **The DDATA topic, not the DBIRTH one.** Both carry the same device level, so
/// either would prove the mapping — but DDATA is the topic an operator will meet
/// again in a SCADA trend, and showing them a topic they will never see twice
/// makes the check harder to repeat.
///
/// Returns the topic error rather than rendering something approximate: a
/// preview that quietly differs from what is published is worse than no preview,
/// because it is a check that passes for the wrong reason.
pub fn mapping_preview(config: &BridgeConfig) -> Result<Vec<MappingRow>, sparkplug_b::TopicError> {
    let node = sparkplug_b::EdgeNode::new(config.group_id.clone(), config.node_id.clone())?;
    config
        .meters
        .iter()
        .map(|meter| {
            Ok(MappingRow {
                meter: meter.meter.as_str().to_string(),
                device_id: meter.device_id.clone(),
                serial: meter.serial.as_str().to_string(),
                topic: node.device_topic(sparkplug_b::MessageType::DData, meter.serial.as_str())?,
                enabled: meter.enabled,
            })
        })
        .collect()
}

/// Values appearing more than once, each reported once.
fn duplicates<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut repeated = std::collections::BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            repeated.insert(value.to_string());
        }
    }
    repeated.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A configuration with nothing missing, used as the base every test mutates
    /// one thing away from. Built through the same fields a caller fills.
    fn sound() -> RawConfig {
        RawConfig {
            client_id: Some("id".into()),
            client_secret: Some("s3cr3t-do-not-print".into()),
            group_id: Some("Group".into()),
            node_id: Some("Node".into()),
            broker_host: Some("broker".into()),
            meters: vec![RawMeter {
                meter_id: Some("meter-a".into()),
                device_id: Some("dev-a".into()),
                serial: Some("9202685".into()),
                enabled: None,
            }],
            ..Default::default()
        }
    }

    fn fields(errors: &ConfigErrors) -> Vec<&str> {
        errors.0.iter().map(|f| f.field.as_str()).collect()
    }

    #[test]
    fn a_sound_configuration_validates() {
        let config = validate(sound()).expect("should validate");
        assert_eq!(config.poll.interval, PERIOD_DEFAULT);
        assert_eq!(config.broker_port, 1883);
        assert_eq!(config.meters.len(), 1);
        assert!(config.meters[0].enabled, "absent enabled means enabled");
    }

    /// AC2, and the reason this story exists. `?` on the first error would report
    /// one of these and send the operator round again for the next.
    ///
    /// FALSIFIED 2026-08-03. Copied from the run — the first draft of this record
    /// quoted a message no run had produced, which is the failure the *copy, do
    /// not write* rule exists for.
    ///
    /// Mutation: `faults.truncate(1)` before returning, which is exactly the
    /// report-the-first-only shape this AC forbids:
    ///
    /// ```text
    /// test app::config::tests::every_fault_is_reported_not_the_first ... FAILED
    /// panicked at crates/smartme-bridge/src/app/config.rs:521:9:
    /// found 1 fault(s), expected at least 3: ["smart-me client id"]
    /// test result: FAILED. 10 passed; 1 failed
    /// ```
    ///
    /// **Ten tests stayed green under it.** Every other test in this module
    /// asserts that *some* fault is present, and one truncated fault is still a
    /// fault — so this is the only test in the file that carries the property.
    #[test]
    fn every_fault_is_reported_not_the_first() {
        let raw = RawConfig {
            client_id: None,
            group_id: None,
            broker_host: None,
            ..sound()
        };
        let errors = validate(raw).expect_err("should be refused");
        let named = fields(&errors);
        assert!(
            named.len() >= 3,
            "found {} fault(s), expected at least 3: {named:?}",
            named.len()
        );
        for expected in ["smart-me client id", "Sparkplug group id", "broker host"] {
            assert!(
                named.contains(&expected),
                "{expected} missing from {named:?}"
            );
        }
    }

    /// Story 5.3 AC4 — what the operator is asked to confirm.
    ///
    /// **Literal strings, deliberately.** Building the expected topic from the
    /// same expression production uses would assert the code against itself —
    /// the defect the Story 4.2 review found in a conformance row, where the
    /// evidence was a test that could not have failed.
    #[test]
    fn the_mapping_preview_shows_the_serial_and_the_exact_topic() {
        let mut raw = sound();
        raw.group_id = Some("Plant".into());
        raw.node_id = Some("Bridge01".into());
        raw.meters = vec![RawMeter {
            meter_id: Some("garage".into()),
            device_id: Some("a1a1a1a1-b2b2-c3c3-d4d4-000000000001".into()),
            serial: Some("9202685".into()),
            enabled: Some(true),
        }];
        let config = validate(raw).expect("validates");

        let rows = mapping_preview(&config).expect("a valid configuration previews");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].meter, "garage");
        assert_eq!(rows[0].serial, "9202685");
        assert_eq!(
            rows[0].device_id, "a1a1a1a1-b2b2-c3c3-d4d4-000000000001",
            "the device id is the half that is easy to cross-wire, because it is \
             a UUID nobody recognises by sight"
        );
        assert_eq!(
            rows[0].topic, "spBv1.0/Plant/DDATA/Bridge01/9202685",
            "the operator must see the topic they will meet again in a trend"
        );
        assert!(rows[0].enabled);
    }

    /// A fault must name what the operator actually types — and since
    /// 2026-08-04 there are two different answers to that.
    ///
    /// **Added because the first implementation of this module failed it**, and
    /// not at this level: it named settings in prose — *"Sparkplug group id"* —
    /// while a first run was fixed by editing an environment variable. The
    /// integration test `startup_banner` caught it, which means the unit tests
    /// here were all green on a message the reader would have had to translate
    /// before acting on. This is that gap closed where the message is built.
    ///
    /// **This test failed on the ADR 0023 rewiring, which is the point of it.**
    /// It still asserted `SMARTME_GROUP_ID`, a variable that had just stopped
    /// doing anything — the same misdirection as the original defect, merely
    /// pointing somewhere new. Both destinations are now asserted, because a
    /// message naming only one kind is a message that is wrong half the time.
    ///
    /// FALSIFIED 2026-08-04 by mutating `Source`'s `Display` to print the bare
    /// key — which is precisely the message that sends a reader looking for an
    /// environment variable called `group_id`:
    ///
    /// ```text
    /// test a_fault_names_where_the_operator_changes_the_setting ... FAILED
    /// config.toml: group_id is what the operator edits; message was:
    ///   - smart-me client secret (environment: SMARTME_CLIENT_SECRET): is missing or empty
    /// ```
    #[test]
    fn a_fault_names_where_the_operator_changes_the_setting() {
        let raw = RawConfig {
            group_id: None,
            node_id: None,
            broker_host: None,
            client_secret: None,
            meters: vec![RawMeter {
                serial: None,
                ..sound().meters.remove(0)
            }],
            ..sound()
        };
        let rendered = format!("{}", validate(raw).expect_err("should be refused"));

        // Settings in the file: named by their key, and by the file, because
        // "group_id" alone reads as an environment variable to anyone who has
        // met one.
        for expected in [
            "config.toml: group_id",
            "config.toml: node_id",
            "config.toml: broker_host",
            // Indexed, so a fault over four meters says WHICH meter.
            "config.toml: meters[0].serial",
        ] {
            assert!(
                rendered.contains(expected),
                "{expected} is what the operator edits; message was:\n{rendered}"
            );
        }

        // And the one thing that is still an environment variable.
        assert!(
            rendered.contains("environment: SMARTME_CLIENT_SECRET"),
            "the credential is the only setting left in the environment, and a \
             message that sends the reader to config.toml for it is wrong; \
             message was:\n{rendered}"
        );

        // The withdrawn variables must not be named at all: a message telling an
        // operator to set SMARTME_GROUP_ID now describes a variable the binary
        // does not read.
        for withdrawn in ["SMARTME_GROUP_ID", "SMARTME_NODE_ID", "SMARTME_BROKER_HOST"] {
            assert!(
                !rendered.contains(withdrawn),
                "{withdrawn} was withdrawn on 2026-08-04 and setting it does \
                 nothing; message was:\n{rendered}"
            );
        }
    }

    /// AC2 again, on the fault that used to escape it.
    ///
    /// The endpoint check lived in `SmartMeClient::new` at startup, so a bad
    /// endpoint was reported only once everything else was already correct — one
    /// more restart, which is what this story exists to remove. The assertion
    /// that matters is not that the endpoint is refused; it is that it is refused
    /// **in the same pass** as an unrelated fault.
    #[test]
    fn a_refused_endpoint_is_reported_alongside_everything_else() {
        let raw = RawConfig {
            api_base: Some("http://not-tls.example".into()),
            group_id: None,
            ..sound()
        };
        let errors = validate(raw).expect_err("a non-TLS endpoint should be refused");
        let named = fields(&errors);
        assert!(
            named.contains(&"smart-me API base"),
            "the endpoint was not checked at all: {named:?}"
        );
        assert!(
            named.contains(&"Sparkplug group id"),
            "the endpoint fault must not short-circuit the rest: {named:?}"
        );
    }

    /// The struct the secret ARRIVES through must not print it either.
    ///
    /// Added after a falsification run leaked it through a panic message: the
    /// derived `Debug` on `RawConfig` rendered `client_secret: Some("...")` in
    /// full. `StoredSecrets` was protected and this one was not, which is what
    /// makes it worth a test rather than a habit.
    #[test]
    fn the_raw_config_debug_never_renders_the_secret() {
        let secret = "s3cr3t-do-not-print";
        let raw = RawConfig {
            client_secret: Some(secret.into()),
            ..sound()
        };
        let debugged = format!("{raw:?}");
        assert!(
            !debugged.contains(secret),
            "leaked through Debug: {debugged}"
        );
        assert!(
            debugged.contains("<redacted>") && debugged.contains("broker_host"),
            "the absence assertion needs the struct to have rendered at all: {debugged}"
        );
    }

    /// ADR 0019, tested where the value enters the process rather than where a
    /// template renders it. A `Debug` derive or a helpful "got {value}" would
    /// defeat this silently.
    #[test]
    fn no_fault_ever_carries_the_secret() {
        let secret = "s3cr3t-do-not-print";
        let raw = RawConfig {
            client_secret: Some(secret.into()),
            broker_port: Some("not-a-port".into()),
            publish_period_secs: Some("99999".into()),
            ..sound()
        };
        let errors = validate(raw).expect_err("should be refused");
        let rendered = format!("{errors}");
        assert!(
            !rendered.contains(secret),
            "the secret reached an error message:\n{rendered}"
        );
        // The absence assertion above is worthless unless the stream carries
        // something — this project has been caught by exactly that shape.
        assert!(
            rendered.contains("broker port"),
            "expected real faults to be present, got:\n{rendered}"
        );
    }

    /// AC3 / ADR 0020. The interesting half is that there is no way to say "off".
    #[test]
    fn the_publish_period_is_bounded_at_both_ends_and_cannot_be_off() {
        for refused in ["0", "1", "4", "301", "86400"] {
            let raw = RawConfig {
                publish_period_secs: Some(refused.into()),
                ..sound()
            };
            let errors = validate(raw).expect_err(&format!("{refused}s should be refused"));
            assert!(
                fields(&errors).contains(&"publish period"),
                "{refused}s was not reported as a period fault"
            );
        }
        for accepted in ["5", "30", "300"] {
            let raw = RawConfig {
                publish_period_secs: Some(accepted.into()),
                ..sound()
            };
            let config = validate(raw).expect("within bounds");
            assert_eq!(
                config.poll.interval,
                Duration::from_secs(accepted.parse().unwrap())
            );
        }
    }

    /// AC5 — impossible to write before the model held a collection, and green
    /// forever if it had been written against a single field.
    #[test]
    fn two_meters_sharing_a_serial_are_refused() {
        let mut raw = sound();
        raw.meters.push(RawMeter {
            meter_id: Some("meter-b".into()),
            device_id: Some("dev-b".into()),
            serial: Some("9202685".into()),
            enabled: Some(false),
        });
        let errors = validate(raw).expect_err("a shared serial should be refused");
        assert!(
            fields(&errors).contains(&"meter serials"),
            "got {:?}",
            fields(&errors)
        );
    }

    /// AC6 — the guard that keeps a plural model from becoming a silent lie.
    #[test]
    fn more_enabled_meters_than_the_runtime_serves_is_refused() {
        let mut raw = sound();
        raw.meters.push(RawMeter {
            meter_id: Some("meter-b".into()),
            device_id: Some("dev-b".into()),
            serial: Some("9202686".into()),
            enabled: Some(true),
        });
        let errors = validate(raw).expect_err("two enabled meters should be refused");
        assert!(
            fields(&errors).contains(&"enabled meters"),
            "got {:?}",
            fields(&errors)
        );
    }

    /// The other half of AC6: the model may hold the meter Guy has not connected.
    #[test]
    fn a_disabled_meter_is_configured_without_being_served() {
        let mut raw = sound();
        raw.meters.push(RawMeter {
            meter_id: Some("not-connected-yet".into()),
            device_id: Some("dev-b".into()),
            serial: Some("9202686".into()),
            enabled: Some(false),
        });
        let config = validate(raw).expect("a disabled extra meter is legal");
        assert_eq!(config.meters.len(), 2);
        assert_eq!(config.meters.iter().filter(|m| m.enabled).count(), 1);
    }

    /// AC7 — the failure this rule exists for is silent, which is why the message
    /// names the consequence and not the rule.
    #[test]
    fn a_serial_with_a_leading_zero_is_refused() {
        let mut raw = sound();
        raw.meters[0].serial = Some("09202685".into());
        let errors = validate(raw).expect_err("a leading zero should be refused");
        let rendered = format!("{errors}");
        assert!(
            rendered.contains("DroppedUndeclaredDevice"),
            "the message must name what it costs, got:\n{rendered}"
        );
    }

    #[test]
    fn a_serial_that_cannot_be_a_topic_level_is_refused() {
        let mut raw = sound();
        raw.meters[0].serial = Some("has/slash".into());
        let errors = validate(raw).expect_err("a topic-illegal serial should be refused");
        assert!(
            fields(&errors).contains(&"meter 0 serial"),
            "got {:?}",
            fields(&errors)
        );
    }

    #[test]
    fn no_meters_at_all_is_refused() {
        let raw = RawConfig {
            meters: vec![],
            ..sound()
        };
        let errors = validate(raw).expect_err("no meters should be refused");
        assert!(fields(&errors).contains(&"meters"));
    }

    #[test]
    fn every_meter_disabled_is_refused() {
        let mut raw = sound();
        raw.meters[0].enabled = Some(false);
        let errors = validate(raw).expect_err("all-disabled should be refused");
        assert!(fields(&errors).contains(&"enabled meters"));
    }
}
