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

// `RUNTIME_METER_LIMIT` lived here until 2026-08-06 (Story 3.1).
//
// It was `1`, and enabling more was REFUSED rather than truncated, because a
// bridge that quietly published one of four meters would look healthy in every
// way a human checks — node online, tags present, values fresh — while three
// were simply absent. The runtime now serves every enabled meter, so the guard
// has been outgrown rather than weakened: nothing is dropped, so there is no
// subset to refuse. The duplicate-serial, duplicate-meter-id and topic-legality
// guards below are unrelated and stay.

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
    /// **Strings, like everything else here.** They were `Option<usize>` and
    /// `Option<u16>` until 2026-08-05, which broke this struct's own contract —
    /// a rule had already been applied, outside `validate`, by whoever filled it
    /// — and both callers applied it as `.parse().ok()`, which DISCARDS failure.
    /// An operator who typed `8O80` (letter O) in the web UI's port box got no
    /// fault, a page saying "Saved", and a setting that had silently vanished.
    pub log_keep: Option<String>,
    pub ui_port: Option<String>,
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

/// Parse an optional whole number, reporting a fault rather than dropping it.
fn number<T: std::str::FromStr>(
    raw: &Option<String>,
    field: &str,
    key: &str,
    faults: &mut Vec<Fault>,
) -> Option<T> {
    let text = present(raw)?;
    match text.parse() {
        Ok(value) => Some(value),
        Err(_) => {
            faults.push(Fault {
                field: field.to_string(),
                source: Some(Source::File(key.to_string())),
                problem: format!(
                    "is not a whole number: {text:?}. It is reported rather than \
                     ignored — a setting that silently vanishes is worse than one \
                     that is refused"
                ),
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
    // The source names the exact key, as every other meter fault does. It said
    // `None` until Story 6.2: the configuration screen binds a fault to its input
    // by `Source::File`, so a fault with no source could only be shown in the
    // lump at the top of the form — for the one setting whose whole purpose is
    // to be checked against a specific row.
    let key = Some(Source::File(format!("meters[{index}].serial")));
    if raw.len() > 1 && raw.starts_with('0') {
        faults.push(Fault {
            field: field.clone(),
            source: key.clone(),
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
    //
    // **Checked even when the Sparkplug identity is itself faulty**, and it was
    // not until 2026-08-05. `node` is `None` whenever `group_id`/`node_id` are
    // missing or malformed, and this whole block was skipped — so `group_id = ""`
    // with `serial = "meter/1"` reported the group id, the operator fixed it and
    // restarted, and only then learnt the serial cannot be a topic level. Two
    // edit-restart cycles, on the one rule AC7 exists for, in the story written
    // to abolish them.
    //
    // A serial's grammar does not depend on the node it hangs under, so a
    // placeholder identity exercises exactly the same rule. It is never used for
    // anything but this check.
    let grammar = node
        .cloned()
        .or_else(|| sparkplug_b::EdgeNode::new("g".to_string(), "n".to_string()).ok());
    if let Some(node) = grammar.as_ref() {
        if let Err(error) = node.device_topic(sparkplug_b::MessageType::DBirth, raw) {
            faults.push(Fault {
                field,
                source: key,
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
                    // Sourced on `group_id` rather than left unbound: a fault
                    // with no source cannot be drawn beside an input, so this
                    // one landed in the lump at the top of the form for a value
                    // the operator types into a specific box.
                    field: "Sparkplug group id / node id".to_string(),
                    source: Some(Source::File("group_id".into())),
                    problem: format!(
                        "cannot form a topic: {error}. Both identifiers are \
                         checked together; the fault may be in either."
                    ),
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
    // AC5 asks that the fault "names BOTH offenders", and it named neither until
    // 2026-08-05: it reported the duplicated *value* once and left an operator
    // with four meters to work out which two rows collided. The offending rows
    // are listed by index and by name, and the fault is sourced on the first of
    // them so a form can draw it beside a box.
    let offenders = |matching: Vec<usize>| -> String {
        matching
            .iter()
            .map(|i| format!("meter {i} ({})", meters[*i].meter.as_str()))
            .collect::<Vec<_>>()
            .join(" and ")
    };
    duplicates(meters.iter().map(|m| m.serial.as_str()))
        .into_iter()
        .for_each(|serial| {
            let rows: Vec<usize> = meters
                .iter()
                .enumerate()
                .filter(|(_, m)| m.serial.as_str() == serial)
                .map(|(i, _)| i)
                .collect();
            let first = rows.first().copied().unwrap_or(0);
            faults.push(Fault {
                field: "meter serials".to_string(),
                source: Some(Source::File(format!("meters[{first}].serial"))),
                problem: format!(
                    "{serial:?} is used by {}; they would share one Sparkplug \
                     device topic and overwrite each other",
                    offenders(rows)
                ),
            })
        });
    duplicates(meters.iter().map(|m| m.meter.as_str()))
        .into_iter()
        .for_each(|meter| {
            let rows: Vec<usize> = meters
                .iter()
                .enumerate()
                .filter(|(_, m)| m.meter.as_str() == meter)
                .map(|(i, _)| i)
                .collect();
            let first = rows.first().copied().unwrap_or(0);
            faults.push(Fault {
                field: "meter ids".to_string(),
                source: Some(Source::File(format!("meters[{first}].meter_id"))),
                problem: format!("{meter:?} is used by {}", offenders(rows)),
            })
        });

    let enabled = meters.iter().filter(|m| m.enabled).count();
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
    // NOT guarded by the credential, and it was until 2026-08-05.
    //
    // The comment above says this check lives here so that a bad endpoint and a
    // missing group id are reported together rather than over two restarts. The
    // guard reinstated exactly that, one field over: on a genuine first run the
    // credential is the MOST likely thing to be absent, so the endpoint fault was
    // withheld until it had been supplied. And it bought nothing —
    // `SmartMeClient::new` never inspects the credentials; every rejection is a
    // function of the base URL and the timeout alone.
    // Parsed HERE rather than by whoever filled `RawConfig`, so a value that is
    // not a number is a fault the operator can see instead of a setting that
    // quietly disappeared.
    let log_keep = number(&raw.log_keep, "log retention", "log_keep", &mut faults);
    let ui_port: Option<u16> = number(&raw.ui_port, "web UI port", "ui_port", &mut faults);
    if ui_port == Some(0) {
        faults.push(Fault {
            field: "web UI port".to_string(),
            source: Some(Source::File("ui_port".into())),
            problem: "is 0, which asks the operating system to pick a port at \
                      random — the address would change at every restart and \
                      nothing could be configured to reach it"
                .to_string(),
        });
    }
    // A PRIVILEGED PORT IS A ONE-WAY DOOR, and that is why it is refused here
    // rather than left to fail at bind time.
    //
    // The image runs as uid 10002, so a port below 1024 cannot be bound. `serve`
    // treats that as "no UI" and lets the bridge publish on — deliberately, since
    // a diagnostic aid must not be able to cause an outage. But `ui_port` is the
    // one setting whose only editor is the web UI: accept 80, restart, and the
    // screen is gone with no way to change the value that removed it, short of
    // hand-editing the file on the volume.
    //
    // So the refusal has to happen while the operator still has a screen to read
    // it on. Note this catches the deterministic case only; a port already in use,
    // or one the reverse proxy is not routing to, has the same effect and cannot
    // be seen from here.
    if let Some(port) = ui_port
        && (1..1024).contains(&port)
    {
        faults.push(Fault {
            field: "web UI port".to_string(),
            source: Some(Source::File("ui_port".into())),
            problem: format!(
                "is {port}. Ports below 1024 are privileged and this bridge runs \
                 unprivileged, so it could never listen there — and since this \
                 screen is the only place the web UI port can be changed, \
                 accepting it would remove the only way to undo it. Use 1024 or \
                 above; the reverse proxy is what publishes a low port to the \
                 outside"
            ),
        });
    }

    //
    // The credential handed to the probe is a placeholder for the same reason:
    // it is never read, and using the real one would make an endpoint fault
    // depend on a value that has nothing to do with it.
    if let Err(error) = SmartMeClient::new(
        api_base.clone(),
        Credentials::ClientCredentials {
            client_id: String::new(),
            client_secret: String::new(),
        },
        Duration::from_secs(10),
    ) {
        faults.push(Fault {
            field: "smart-me API base".to_string(),
            source: Some(Source::File("api_base".into())),
            // `SmartMeError`'s Display never embeds credentials — and it cannot
            // embed these, which are empty.
            problem: format!("was refused: {error}"),
        });
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
        log_keep,
        ui_port,
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

    /// `ui_port` is the only setting whose sole editor is the screen it governs,
    /// so a value the process cannot bind is a one-way door: accept 80, restart,
    /// and there is no screen left to change 80 back.
    ///
    /// The assertion names the *source* `ui_port`, not merely "an error".
    /// `validate` has a dozen other ways to fail, and `is_err()` would be
    /// satisfied by any of them — the shape that let a duplicate-serial fault
    /// point at the wrong row.
    ///
    /// FALSIFIED 2026-08-06 by widening the guard to `(1..1).contains(&port)`,
    /// so no port is privileged. Copied from the run:
    ///
    /// ```text
    /// test app::config::tests::a_privileged_ui_port_is_refused_while_a_screen_still_exists ... FAILED
    ///
    /// thread '…a_privileged_ui_port_is_refused_while_a_screen_still_exists' (353) panicked at
    /// crates/smartme-bridge/src/app/config.rs:883:18:
    /// a port an unprivileged process cannot bind must be refused: BridgeConfig { …
    /// client_secret: "<redacted>", … ui_port: Some(1) }
    ///
    /// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 166 filtered out
    /// ```
    ///
    /// It dies on port 1, the first case, at the `expect_err` — not at the
    /// source-naming assertion below it, which is therefore proved only against
    /// the guard being present at all. (The dump also shows the hand-written
    /// `Debug` doing its job: `client_secret: "<redacted>"`.)
    #[test]
    fn a_privileged_ui_port_is_refused_while_a_screen_still_exists() {
        for port in ["1", "80", "443", "1023"] {
            let raw = RawConfig {
                ui_port: Some(port.into()),
                ..sound()
            };
            let errors = validate(raw)
                .expect_err("a port an unprivileged process cannot bind must be refused");
            assert!(
                errors
                    .0
                    .iter()
                    .any(|f| f.source == Some(Source::File("ui_port".into()))),
                "port {port} cannot be bound by an unprivileged process, and accepting \
                 it removes the only screen that could undo it; got {:?}",
                fields(&errors)
            );
        }

        // The other side, and it is the one that makes the test mean something: a
        // guard that refused every port would pass the half above and make the UI
        // unconfigurable.
        for port in ["1024", "8080", "65535"] {
            let raw = RawConfig {
                ui_port: Some(port.into()),
                ..sound()
            };
            let config = validate(raw)
                .unwrap_or_else(|e| panic!("port {port} is bindable and must be accepted: {e:?}"));
            assert_eq!(config.ui_port, Some(port.parse().expect("a u16")));
        }
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

    /// **Story 3.1 AC1, and this test asserted the OPPOSITE until 2026-08-06.**
    ///
    /// It was `more_enabled_meters_than_the_runtime_serves_is_refused`, and it was
    /// right: story 5.1's AC6 refused a configuration enabling more meters than
    /// the runtime served, because publishing one of four while looking healthy is
    /// the exact failure this project exists to prevent. The guard has been
    /// outgrown, not weakened — nothing is truncated now, because everything is
    /// served.
    ///
    /// Four meters, which is the fleet the deployment actually has.
    ///
    /// FALSIFIED 2026-08-06 by restoring the `enabled > 1` fault in `validate`.
    /// Copied from the run:
    ///
    /// ```text
    /// test app::config::tests::every_enabled_meter_is_accepted_because_every_one_is_served ... FAILED
    ///
    /// thread '…every_enabled_meter_is_accepted_because_every_one_is_served' (57) panicked at
    /// crates/smartme-bridge/src/app/config.rs:1223:13:
    /// the runtime serves every enabled meter since Story 3.1, so four of them is a
    /// configuration and not a fault: ConfigErrors([Fault { field: "enabled meters",
    /// source: None, problem: "MUTATION" }])
    ///
    /// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 166 filtered out
    /// ```
    ///
    /// It dies on the `validate` itself, not on the count assertions below it —
    /// so those two are proved only against the fault being absent, which is what
    /// they are for.
    #[test]
    fn every_enabled_meter_is_accepted_because_every_one_is_served() {
        let mut raw = sound();
        for (n, serial) in [(2, "9202686"), (3, "9202687"), (4, "9202688")] {
            raw.meters.push(RawMeter {
                meter_id: Some(format!("meter-{n}")),
                device_id: Some(format!("dev-{n}")),
                serial: Some(serial.into()),
                enabled: Some(true),
            });
        }
        let config = validate(raw).unwrap_or_else(|e| {
            panic!(
                "the runtime serves every enabled meter since Story 3.1, so four of \
                 them is a configuration and not a fault: {e:?}"
            )
        });
        // The COUNT, not merely "it validated": a `validate` that dropped the
        // extra meters would satisfy the line above and reintroduce, silently,
        // the truncation story 5.1 refused.
        assert_eq!(config.meters.len(), 4);
        assert_eq!(config.meters.iter().filter(|m| m.enabled).count(), 4);
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
