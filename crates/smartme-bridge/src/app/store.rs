//! Where the configuration rests, and what it costs to read it back wrong
//! (Story 5.2, [ADR 0023]).
//!
//! # One file, and no secret in it
//!
//! `config.toml` holds the whole configuration. The smart-me credential is not
//! in it and never will be: it arrives from the environment, is held for the
//! lifetime of the process, and never descends to disk.
//!
//! That is what makes this module boring, and the boringness is the point. There
//! is no second file to keep in step, no mode to verify, and no `sudo cat` habit
//! to form — checking which topic a meter maps to has never needed a privilege
//! and now provably cannot.
//!
//! [ADR 0022] designed the opposite (a `0600` `secrets.toml` beside this one) and
//! is superseded. It was written against FR46 without checking **NFR12**, which
//! had said *"credentials only in `.env`/env vars"* since before either existed.
//!
//! # The credential is a pair, and travels as one
//!
//! [`Credential`] carries `client_id` **and** `client_secret` together. Two
//! adjacent `Option<String>` parameters would be swappable at a call site, and a
//! swapped pair fails as an authentication rejection from the smart-me API — which
//! reads as an outage of the upstream service rather than a configuration fault,
//! and is diagnosed accordingly and at length.
//!
//! # An older file must not be read by guesswork
//!
//! [`StoredConfig`] is `deny_unknown_fields` and carries a [`SCHEMA_VERSION`].
//! Serde's defaults are the trap: unknown fields are ignored and missing ones take
//! `Default`, so a renamed field would read as *absent*, take its default, and the
//! bridge would start on a configuration nobody wrote — publishing at 30 s because
//! the period silently reverted. Refusing was the only honest answer until a
//! migration existed to be the other one; since [ADR 0040] one does, for the steps
//! it names, and everything else is still refused.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
//! [ADR 0022]: ../../../docs/adr/0022-secrets-rest-in-a-separate-0600-file.md

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::config::{ConfigErrors, Fault, RawConfig, RawMeter};
use crate::app::supervisor::BridgeConfig;
use crate::persist;

/// The shape of what is on disk. Bumped whenever a field is added, renamed or
/// removed — see the module docs for why "read it and hope" is not available.
///
/// **4 since 2026-08-04 (Story 6.1)**: `ui_port`. **3** added `mapping_confirmed`. **2** added `log_dir`
/// and `log_keep`, moved in from the environment.
///
/// Both bumps were made even though the added fields are optional-with-default,
/// so an older file would have parsed. An exception made once to "bump whenever
/// a field is added" is how the guarantee stops being one — and version 3's
/// default is the one that must not be got wrong: an unrecognised older file
/// reads as **unconfirmed**, which costs one click, where the other direction
/// would publish a mapping nobody had looked at.
pub const SCHEMA_VERSION: u32 = 5;

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.toml")
}

/// The smart-me credential, as it arrives from the environment.
///
/// Deliberately **not** `Serialize`: there is no code path that could write this
/// to disk, because there is no derive that would let it. The type system carries
/// the rule so nobody has to remember it.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// Hand-written so that `{:?}` cannot leak the secret.
///
/// A `#[derive(Debug)]` here would put the credential into every error that
/// formats a struct containing one — which is not hypothetical: Story 1.6's
/// review found exactly that, a panic message rendering
/// `client_secret: Some("…")` in full from a derive nobody had looked at.
/// Where a secret lives, the derive is the defect.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("client_id", &self.client_id.as_ref().map(|_| "<redacted>"))
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// The configuration, as stored. **Every setting the bridge has lives here** —
/// what this struct does not hold, no source supplies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoredConfig {
    pub schema_version: u32,
    pub group_id: String,
    pub node_id: String,
    pub broker_host: String,
    pub broker_port: u16,
    pub publish_period_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    /// Where rotated log files are written. Absent means console only.
    ///
    /// **File logging stays opt-in.** A default path would give the two chaos
    /// tests that spawn the real binary and read its output a file to write into,
    /// which is how a comfort feature acquires the power to break the tests that
    /// guard the product's honesty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_dir: Option<String>,
    /// How many daily-rotated files to keep. Absent means [`DEFAULT_LOG_KEEP`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_keep: Option<usize>,
    /// Whether a human has looked at the meter→topic mapping and said it is
    /// right (FR25, Story 5.3).
    ///
    /// **Absent means `false`**, and the direction is the decision: an older
    /// file, or one somebody assembled without reading this, costs one click.
    /// Defaulting the other way would publish a mapping nobody had looked at
    /// into a namespace a Sparkplug host persists.
    ///
    /// **A caller cannot set this through [`save`].** Confirmation is a human
    /// act about a *specific* mapping, so a writer able to assert it in the same
    /// call that changes the mapping would make the guard decorative. [`save`]
    /// computes it; [`confirm`] is the only way to make it true.
    #[serde(default)]
    pub mapping_confirmed: bool,
    /// Port the embedded web UI listens on, inside the container.
    ///
    /// Absent means [`crate::ui::DEFAULT_PORT`]. It has a default because the
    /// first run has no file to read one from — and that is the run that needs
    /// the UI most.
    ///
    /// **Changing it costs a new session** by the same argument as the broker: a
    /// listener cannot move without dropping what is connected to it. See
    /// `app::reconfigure`, which will not compile until this is classified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_port: Option<u16>,
    /// When this configuration was first written, in UTC epoch-ms ([ADR 0039]).
    ///
    /// **Stamped by [`save`] only when there was no readable file**, and carried
    /// over on every later write. A file that predates ADR 0039 never acquires one:
    /// there is nothing true to write, and the screen says "unknown" rather than
    /// pretending.
    ///
    /// **NEVER the file's mtime.** A `docker cp`, a restore from backup, a `touch`
    /// or an image update that rewrites the volume all move it — it would be a
    /// plausible date that is not the date of anything this bridge did, on the one
    /// screen whose job is to orient somebody at three in the morning.
    ///
    /// [ADR 0039]: ../../../docs/adr/0039-the-configuration-remembers-when-it-was-written-and-which-meters-matter.md
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_ms: Option<i64>,
    /// When the settings last actually changed, in UTC epoch-ms ([ADR 0039]).
    ///
    /// **Moves only when the settings differ from the stored ones.** A Save that
    /// changes nothing is not a change; the alternative would make this field mean
    /// "last time somebody pressed a button", which is the distinction story 6.3
    /// drew between `last_changed_at` and `last_published_at`, one layer down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_change_ms: Option<i64>,
    pub meters: Vec<StoredMeter>,
}

/// Rotated log files kept when the configuration does not say. Rotation is
/// daily, so this is a retention window in days — the seven Guy chose on
/// 2026-08-01.
pub const DEFAULT_LOG_KEEP: usize = 7;

/// One meter, as stored. Note there is **no secret here** — a meter's identity
/// is not sensitive, only the account that reads it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoredMeter {
    pub meter_id: String,
    pub device_id: String,
    pub serial: String,
    pub enabled: bool,
    /// Whether this is one of the meters the operator actually cares about
    /// ([ADR 0039], FR35).
    ///
    /// **The operator's statement, because nowhere else knows.** smart-me's
    /// `Device` payload carries an id, a name, a serial and two numbers — no make
    /// and no model — so the bridge cannot tell a Kamstrup from a Telstar, and a
    /// flag deduced from a meter name would be an assertion about hardware with no
    /// evidence behind it.
    ///
    /// **Absent means `false`**, which is the harmless direction: an older file
    /// claims no priorities rather than inventing some.
    ///
    /// [ADR 0039]: ../../../docs/adr/0039-the-configuration-remembers-when-it-was-written-and-which-meters-matter.md
    #[serde(default)]
    pub priority: bool,
}

fn fault(field: &str, problem: String) -> Fault {
    Fault {
        field: field.to_string(),
        source: None,
        problem,
    }
}

/// Is there a stored configuration at all?
///
/// **This is the seam between two states that must not be merged.** Absent means
/// a first run: the bridge comes up, serves the UI, and puts nothing on the wire.
/// Present-but-invalid means a refusal to publish (Story 5.1, amended by
/// ADR 0026 — the process stays up and serves the repair screen). Collapsing them
/// either bricks the first run or lets a corrupt file be mistaken for a fresh
/// install and silently overwritten.
pub fn exists(dir: &Path) -> bool {
    // `try_exists`, NOT `Path::exists()`.
    //
    // `Path::exists()` returns `false` on ANY metadata error, permission
    // included — so a `/data` bind-mounted from a directory the container's uid
    // cannot traverse made a fully configured, confirmed bridge report itself as
    // a first run, log *"no configuration yet"* at INFO, serve the first-run
    // screen and publish nothing. That is exactly the merge this function exists
    // to prevent, performed by the function itself. The chown to uid 10002 is a
    // documented deployment step, which is to say it is one people forget.
    //
    // An error is treated as PRESENT: it is the direction that refuses rather
    // than overwrites.
    match std::fs::exists(config_path(dir)) {
        Ok(present) => present,
        Err(error) => {
            tracing::warn!(
                path = %config_path(dir).display(),
                %error,
                "cannot tell whether a configuration exists; assuming it does, so \
                 nothing overwrites it. This is usually the state directory's \
                 ownership — it must belong to the uid the container runs as"
            );
            true
        }
    }
}

/// Read the file, and nothing more.
///
/// Split out from [`load`] for one reason: **`log_dir` and `log_keep` are in the
/// file, so the file has to be read before the logging subscriber can be built.**
/// The caller therefore needs the stored form before it has anywhere to trace a
/// fault to — see `main.rs`, which holds the result and reports it once a
/// subscriber exists.
pub fn read(dir: &Path) -> Result<StoredConfig, ConfigErrors> {
    let mut faults = Vec::new();

    let config: Option<StoredConfig> = match persist::load(&config_path(dir)) {
        Ok(config) => Some(config),
        // THE TWO FAILURES HAVE DIFFERENT REPAIRS, and only one of them is in the
        // browser ([ADR 0026]). Telling an operator to correct a field, when what
        // is wrong is that the process cannot read the directory at all, sends
        // them to a form that can never succeed.
        //
        // `InvalidData` is the content: `persist::load` wraps every TOML error in
        // it, and `read_to_string` uses it for bytes that are not UTF-8. Both are
        // repairable by writing the file again, which the form does. Anything else
        // — permissions above all — is the filesystem, and the repair is on the
        // host.
        Err(error) if error.kind() != std::io::ErrorKind::InvalidData => {
            faults.push(fault(
                "stored configuration",
                format!(
                    "{} could not be read at all: {error}. This is almost always the \
                     state directory's ownership — it must belong to the uid the \
                     container runs as (10002), so the fix is `chown -R 10002:10002` \
                     on the directory mounted at {}, on the host. Nothing here can \
                     repair it, because nothing here can write there either",
                    config_path(dir).display(),
                    dir.display()
                ),
            ));
            None
        }
        Err(error) => {
            faults.push(fault(
                "stored configuration",
                format!(
                    "{} could not be read: {error}. Refusing to publish beats publishing \
                     on defaults nobody chose — correct it in the form below and save, \
                     which replaces the file wholesale",
                    config_path(dir).display()
                ),
            ));
            None
        }
    };

    // THE VERSION GATE, and since [ADR 0040] it has a door in it.
    //
    // A version this build can migrate is migrated in memory and read; anything
    // else is still refused rather than read by guesswork, which is what the gate
    // was built for. The file itself is not rewritten here — see `migrate`.
    let mut config = config;
    if let Some(stored) = config.as_mut() {
        match migrate(stored) {
            Ok(()) => {}
            Err(version) => faults.push(fault(
                "stored configuration",
                format!(
                    "was written by schema version {version}, this build reads \
                     {SCHEMA_VERSION}, and no migration exists for that step — so it is \
                     refused rather than read by guesswork. An unrecognised field would \
                     otherwise take its default and the bridge would run on settings \
                     nobody wrote"
                ),
            )),
        }
    }

    if !faults.is_empty() {
        return Err(ConfigErrors(faults));
    }

    Ok(config.expect("no faults means the configuration was read"))
}

/// Read the stored configuration into the same [`RawConfig`] the environment
/// produced before it, so a single [`crate::app::config::validate`] governs it.
///
/// The credential is passed in rather than read here: this module knows about a
/// file, and the environment is not its business.
pub fn load(dir: &Path, credential: Credential) -> Result<RawConfig, ConfigErrors> {
    Ok(into_raw(read(dir)?, credential, dir))
}

/// Join the file half to the environment half. **This is the only place the two
/// sources meet**, and they meet without overlapping: no field is supplied by
/// both, so there is nothing to arbitrate and no way for one to silently
/// override the other (ADR 0023 §4).
pub fn into_raw(config: StoredConfig, credential: Credential, dir: &Path) -> RawConfig {
    RawConfig {
        api_base: config.api_base,
        client_id: credential.client_id,
        client_secret: credential.client_secret,
        group_id: Some(config.group_id),
        node_id: Some(config.node_id),
        broker_host: Some(config.broker_host),
        broker_port: Some(config.broker_port.to_string()),
        state_dir: Some(dir.display().to_string()),
        publish_period_secs: Some(config.publish_period_secs.to_string()),
        log_dir: config.log_dir,
        log_keep: config.log_keep.map(|v| v.to_string()),
        ui_port: config.ui_port.map(|v| v.to_string()),
        meters: config
            .meters
            .into_iter()
            .map(|m| RawMeter {
                meter_id: Some(m.meter_id),
                device_id: Some(m.device_id),
                serial: Some(m.serial),
                enabled: Some(m.enabled),
                priority: Some(m.priority),
            })
            .collect(),
    }
}

/// Derive the stored shape from a configuration that has **been validated**.
///
/// # This is the only way a writer should build a `StoredConfig`
///
/// The web form used to re-derive the numbers from its own raw strings, and the
/// two paths disagreed on exactly the case that matters: an operator who clears
/// the publish-period box submits nothing, `validate` reads that as *unset* and
/// supplies `PERIOD_DEFAULT`, and the re-derivation read it as
/// `"".parse().ok().unwrap_or_default()` — **zero**. The browser said "Saved",
/// the file said `publish_period_secs = 0`, and the next start refused it. With
/// [Story 6.1 AC1] refusing to serve a UI for an invalid file, the operator was
/// left with a crash-looping container and a hand-edit over SSH as the only
/// repair. Reachable in one click, through the supported path.
///
/// Going through the validated struct makes that class impossible rather than
/// unlikely: whatever `validate` returned is what reaches the disk, defaults
/// resolved and all.
///
/// `mapping_confirmed` is set to `false` and it does not matter what it is set
/// to — [`save`] discards the caller's value and computes its own (Story 5.3
/// AC3). `false` merely says out loud that a conversion is not where a
/// confirmation can come from.
///
/// **`api_base` is written resolved**, not left absent. The file then records
/// the endpoint actually in force rather than relying on a default the operator
/// cannot see — and a value that came from a default reads back through
/// `validate` unchanged.
///
/// [Story 6.1 AC1]: ../../../_bmad-output/implementation-artifacts/6-1-the-server-exists-in-every-state-the-bridge-can-be-in.md
impl From<&BridgeConfig> for StoredConfig {
    fn from(config: &BridgeConfig) -> Self {
        // EXHAUSTIVE, deliberately — no `..`. A new field on `BridgeConfig`
        // breaks this line, and breaking it is the point: the writer must say
        // whether the new setting is persisted, rather than silently dropping it
        // on the next save. `reconfigure::classify` is guarded the same way.
        let BridgeConfig {
            api_base,
            credentials: _,  // ADR 0023: never on disk.
            http_timeout: _, // not configurable; hardcoded by `validate`.
            meters,
            group_id,
            node_id,
            broker_host,
            broker_port,
            bd_seq_path: _, // derived from the state directory, not stored.
            poll,
            policy: _, // not configurable; hardcoded by `validate`.
            log_dir,
            log_keep,
            ui_port,
        } = config;

        Self {
            schema_version: SCHEMA_VERSION,
            group_id: group_id.clone(),
            node_id: node_id.clone(),
            broker_host: broker_host.clone(),
            broker_port: *broker_port,
            publish_period_secs: poll.interval.as_secs(),
            api_base: Some(api_base.clone()),
            log_dir: log_dir.clone(),
            log_keep: *log_keep,
            mapping_confirmed: false,
            // NOT the caller's to supply, exactly like `mapping_confirmed` above:
            // `save` computes both from the file it is about to overwrite. A
            // conversion cannot know whether this configuration already exists.
            created_ms: None,
            last_change_ms: None,
            ui_port: *ui_port,
            meters: meters
                .iter()
                .map(|m| StoredMeter {
                    meter_id: m.meter.as_str().to_string(),
                    device_id: m.device_id.clone(),
                    serial: m.serial.as_str().to_string(),
                    enabled: m.enabled,
                    priority: m.priority,
                })
                .collect(),
        }
    }
}

/// Do these two configurations publish the same thing?
///
/// Order does not matter; multiplicity does.
///
/// Reordering the meters in a form changes nothing about what reaches the wire,
/// and treating it as a change would make the confirmation lapse for no reason an
/// operator could see — which is how a guard becomes noise and then becomes
/// ignored.
///
/// **Written as `a.len() == b.len() && a.iter().all(|m| b.contains(m))` until a
/// review caught it on 2026-08-04.** That is a SUBSET test, and equal length only
/// makes a subset an equality when the left side has no duplicates. With a meter
/// listed twice in the stored file, `[M, M]` compared against `[M, N]` returned
/// true — so a brand-new device inherited a confirmation given for a mapping it
/// was never part of, and was born on the wire under it. Reachable through this
/// module's own public API, with no hand-editing.
///
/// **This is NOT the rule `crate::app::reconfigure::classify` uses**, and the
/// difference is deliberate rather than an oversight: `classify` compares only
/// *enabled* meters keyed by id, because it answers "what must the wire be told";
/// this compares the whole list, because it answers "is this the mapping a human
/// looked at". Editing a disabled meter's serial changes nothing on the wire and
/// still deserves a fresh look.
///
/// # The node identity is part of the mapping
///
/// **It compared meters alone until 2026-08-06**, while `ui::screens`'
/// `mapping_fingerprint` — the value that binds the operator's click to what the
/// screen showed them — had always included `group_id` and `node_id`. Two rules
/// for one question, disagreeing.
///
/// The gap was reachable through the path the manual recommends: confirm the
/// mapping, then correct the node id. `save` carried the confirmation over,
/// `classify` called it a new session, the screen honestly said "waiting for a
/// restart", and the bridge came back publishing into a namespace no human had
/// ever seen — which is precisely the harm FR25 exists to prevent, *"the only
/// guard against a mis-map the machine cannot detect"*.
///
/// Every identifier here appears in every topic the bridge publishes. Changing
/// one changes where every device lands, so it is exactly as much a mapping
/// change as swapping a serial.
/// Visible to the crate for ONE reason: `ui::screens` derives the fingerprint that
/// binds the operator's click, and the two must answer the same question. They
/// disagreed for a month because nothing could compare them. See
/// `ui::screens::tests::the_withdrawal_rule_and_the_fingerprint_answer_the_same_question`.
pub(crate) fn same_mapping(a: &StoredConfig, b: &StoredConfig) -> bool {
    mapping_projection(a) == mapping_projection(b)
}

/// THE MAPPING, projected — the one answer to *"what did a human vouch for?"*
/// (story 3.4 AC5, [#64]).
///
/// Two readers ask that question: [`same_mapping`] (must this save withdraw the
/// confirmation?) and `ui::screens::mapping_fingerprint` (does this click
/// confirm what was shown?). They diverged once for a month over the node
/// identity, and [#64] recorded the mechanism by which they would diverge
/// again: one read a meter as a whole value (a new field counts automatically),
/// the other formatted four fields by name (a new field is invisible). Story
/// 3.4 — the next story to touch `StoredMeter` — was its deadline.
///
/// The remedy is `reconfigure::classify`'s: an **exhaustive destructure**, so a
/// field added to either struct stops the build here until somebody classifies
/// it. A `#[derive]` folding in every field was rejected in the issue for good
/// reason: it would withdraw confirmations on changes that have nothing to do
/// with the mapping, the same defect wearing the fix.
///
/// **A `field: _` in the destructure IS a classification** — it says
/// NOT-MAPPING — and it must be written into the lists below with its reason,
/// like every other. The build-stop makes the question unavoidable; only the
/// answer's honesty is still a human's (the review of story 3.4 named this
/// residual: the mechanism forces AN answer, not a considered one, and
/// `the_projection_decides_membership_for_every_field` enumerates by hand, so
/// a thoughtless `_` must also be added there to be caught).
///
/// # The classification, field by field
///
/// **MAPPING** (a change re-attributes what lands where, or under which name —
/// the confirmation must be withdrawn and re-given):
/// - `group_id`, `node_id` — both appear in every topic the bridge publishes;
///   changing one changes where every device lands (the 2026-08-06 repair).
/// - `meters[].meter_id` — the name in the Sparkplug metric path.
/// - `meters[].device_id` — which cloud device feeds the row.
/// - `meters[].serial` — the device level of the topic, and the identity
///   ADR 0029 binds every response to.
/// - `meters[].enabled` — whether the row reaches the wire at all; and editing
///   a DISABLED meter still deserves a fresh look, so disabled rows project
///   like any other (the position [`same_mapping`]'s doc has always held).
///
/// **NOT MAPPING** (the wire's meter→topic attribution is untouched):
/// - `schema_version` — how the file is written, not what is published.
/// - `broker_host`, `broker_port` — where the wire goes, not what lands on it;
///   moving brokers is `reconfigure::classify`'s business (a new session), and
///   a human vouches for the mapping, not for the transport.
/// - `publish_period_secs` — cadence, not identity.
/// - `api_base` — where readings are fetched from; identity is bound per
///   response by ADR 0029's serial check, not by origin.
/// - `log_dir`, `log_keep`, `ui_port` — operational comfort.
/// - `mapping_confirmed` — definitionally: the vouch cannot be part of what is
///   vouched for.
/// - The ORDER of meter rows — reordering changes nothing about what reaches
///   the wire; rows are sorted here, which is also what makes two projections
///   comparable as multisets.
pub(crate) fn mapping_projection(stored: &StoredConfig) -> MappingProjection {
    let StoredConfig {
        schema_version: _,
        group_id,
        node_id,
        broker_host: _,
        broker_port: _,
        publish_period_secs: _,
        api_base: _,
        log_dir: _,
        log_keep: _,
        mapping_confirmed: _,
        ui_port: _,
        // Dates are not mapping: they say WHEN this file was written, never WHAT it
        // publishes, so a stamp must not withdraw a confirmation.
        created_ms: _,
        last_change_ms: _,
        meters,
    } = stored;
    let mut rows: Vec<(String, String, String, bool)> = meters
        .iter()
        .map(|m| {
            let StoredMeter {
                meter_id,
                device_id,
                serial,
                enabled,
                // NOT part of the mapping: marking a meter as one that matters
                // changes nothing about what is published for it, so it must not
                // withdraw the mapping confirmation and cost a click.
                priority: _,
            } = m;
            (
                meter_id.clone(),
                device_id.clone(),
                serial.clone(),
                *enabled,
            )
        })
        .collect();
    rows.sort();
    MappingProjection {
        node: (group_id.clone(), node_id.clone()),
        rows,
    }
}

/// What [`mapping_projection`] yields: comparable exactly (that is
/// [`same_mapping`]), and serialisable canonically (that is the fingerprint).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MappingProjection {
    node: (String, String),
    rows: Vec<(String, String, String, bool)>,
}

impl MappingProjection {
    /// The canonical byte form the fingerprint hashes — INJECTIVE since the
    /// story 3.4 review, by length-prefixing every field.
    ///
    /// The separator scheme it replaces was ambiguous: `\u{1f}` and `\u{1e}`
    /// are legal inside every field they delimited (the topic grammar refuses
    /// only `/`, `+`, `#`), so two mappings that [`same_mapping`] calls
    /// DIFFERENT could hash identically — `("a\u{1f}b", "")` and `("a", "b")`
    /// emitted the same bytes, and a group id containing `\u{1e}` could
    /// impersonate a node boundary. Exotic names, but the fingerprint exists
    /// precisely to catch an interleaved write, and a guard with forgeable
    /// bytes is a guard with a hole. A length prefix disambiguates every
    /// boundary; equality of canonicals is now equality of projections.
    ///
    /// Safe to evolve (this change included): the fingerprint is never stored,
    /// so its byte form only ever lives between one rendered page and its
    /// submission. One consequence, accepted and fail-closed: a confirm page
    /// held open across THIS upgrade answers 409 once ("the configuration
    /// changed between the mapping you were shown and this click") and the
    /// operator looks again — the same one-click cost the sort-key change in
    /// this commit can produce for names carrying control characters.
    pub(crate) fn canonical(&self) -> String {
        fn sized(value: &str) -> String {
            format!("{}:{value}", value.len())
        }
        let rows: Vec<String> = self
            .rows
            .iter()
            .map(|(meter_id, device_id, serial, enabled)| {
                format!(
                    "{}\u{1f}{}\u{1f}{}\u{1f}{enabled}",
                    sized(meter_id),
                    sized(device_id),
                    sized(serial)
                )
            })
            .collect();
        format!(
            "{}\u{1e}{}\u{1e}{}",
            sized(&self.node.0),
            sized(&self.node.1),
            rows.join("\u{1e}")
        )
    }
}

/// What a caller stands to destroy by overwriting the stored configuration.
///
/// **The distinction this carries is the whole point.** "Cannot be read" is not
/// one state but two, and refusing both locked the operator out of the screen
/// that exists to repair the first:
///
/// - a file this build has fully diagnosed as broken — bad TOML, a field it does
///   not know, a version older than its own — is exactly what the `Misconfigured`
///   screen renders, fault by fault, into a form. An explicit submission from
///   that form **is** the repair, and refusing it leaves no way out that does not
///   involve a shell on the volume;
/// - a file whose contents this build cannot account for — unreadable bytes, or a
///   schema *newer* than its own — is somebody else's. Overwriting it drops
///   settings this build cannot even represent.
#[derive(Debug, PartialEq, Eq)]
enum Overwrite {
    /// Nothing is there, or what is there is broken in a way this build
    /// understands and a form can replace.
    IsTheRepair,
    /// Refuse, and say what is being protected.
    WouldDestroy(String),
}

/// Decide which of the two [`Overwrite`] cases a directory is in.
///
/// Note this deliberately does **not** consult [`read`]. `read` collapses every
/// failure into one `Err`, and the collapse is what caused the lock-out: a
/// syntax error and a file from a future image were treated identically.
fn overwrite(dir: &Path) -> Overwrite {
    let path = config_path(dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Absent is not a state to protect: there is nothing to destroy.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Overwrite::IsTheRepair;
        }
        Err(error) => {
            return Overwrite::WouldDestroy(format!(
                "{} could not be read at all ({error}), so there is no telling what \
                 overwriting it would throw away. This is usually the state directory's \
                 ownership — it must belong to the uid the container runs as",
                path.display()
            ));
        }
    };

    // Only the version is probed, and **leniently on purpose**: a file written by
    // a newer image carries fields `StoredConfig`'s `deny_unknown_fields` rejects,
    // so parsing it as a `StoredConfig` would fail and hide the very thing that has
    // to be detected. A struct with one field and no `deny_unknown_fields` reads the
    // version out of a document whose remainder this build knows nothing about.
    #[derive(Deserialize)]
    struct Probe {
        schema_version: u32,
    }

    match toml::from_str::<Probe>(&text) {
        Ok(Probe { schema_version }) if schema_version > SCHEMA_VERSION => {
            Overwrite::WouldDestroy(format!(
                "{} was written by schema version {schema_version} and this build writes \
                 {SCHEMA_VERSION}, so it comes from a NEWER image. It was NOT overwritten: \
                 doing so would silently drop settings this build cannot represent. Roll \
                 the image forward again, or move the file aside deliberately",
                path.display()
            ))
        }
        // Everything else — an older version, an unknown field, unparseable TOML, no
        // version at all — is a file this build can say is wrong. A document too
        // damaged to yield even a version number carries no evidence that a newer
        // image owns it, and the operator submitting the form has seen the fault.
        _ => Overwrite::IsTheRepair,
    }
}

/// Bring a parsed configuration up to this build's schema, in memory ([ADR 0040]).
///
/// **Every migrated field takes the default its own ADR argued for**, which is why
/// this can be a version stamp and nothing more: `serde` has already filled
/// `created_ms`, `last_change_ms` and `priority` with the values ADR 0039 chose —
/// no creation date, no change date, nothing marked as mattering. There is no
/// guesswork here, which is the condition the module's refusal rule set.
///
/// **In memory, never on disk.** `read` is called by every screen render; rewriting
/// the file here would make a page view a disk write and hand a read-only surface
/// the power to change what is on disk — during, for instance, the incident the
/// operator opened the page to diagnose. [`save`] stamps the constant, and that is
/// where the file changes.
///
/// Returns the offending version when the step is one this build cannot make:
/// anything from the future, and anything below the oldest migration written.
///
/// [ADR 0040]: ../../../docs/adr/0040-the-first-schema-migration.md
fn migrate(config: &mut StoredConfig) -> Result<(), u32> {
    /// The oldest version this build can still read. Below it, no migration was
    /// ever written — and writing one for a file nobody has would be code with no
    /// evidence behind it.
    const OLDEST_MIGRATABLE: u32 = 4;

    match config.schema_version {
        v if v == SCHEMA_VERSION => Ok(()),
        v if (OLDEST_MIGRATABLE..SCHEMA_VERSION).contains(&v) => {
            // 4 -> 5 ([ADR 0039]): three fields added, all optional-with-default,
            // and the defaults are the honest answers rather than convenient ones.
            // Nothing to compute; the stamp is the migration.
            config.schema_version = SCHEMA_VERSION;
            Ok(())
        }
        // Above this build, or below the oldest migration.
        v => Err(v),
    }
}

/// Do these two configurations hold the same SETTINGS?
///
/// **Not `same_mapping`, and the difference is the point.** That one asks whether
/// what is published changed, so that a confirmation can be withdrawn. This one
/// asks whether anything the operator can set changed, so that a date can stay
/// put: moving the broker port is not a mapping change and it is certainly a
/// change.
///
/// Exhaustive on purpose — no `..`. A new setting breaks this line, and its author
/// has to say whether editing it counts as changing the configuration.
fn same_settings(a: &StoredConfig, b: &StoredConfig) -> bool {
    settings(a) == settings(b)
}

/// The settings half of a stored configuration — everything an operator can edit,
/// and nothing the bridge writes about it.
///
/// **A named type because clippy refused the tuple**, and the lint was right the way
/// it was right about `record_at`'s eight arguments in story 6.4: a ten-element
/// tuple of borrows is a shape nobody can read, and the fields have names already.
#[derive(PartialEq, Eq)]
struct SettingsView<'a> {
    group_id: &'a str,
    node_id: &'a str,
    broker_host: &'a str,
    broker_port: u16,
    publish_period_secs: u64,
    api_base: Option<&'a str>,
    log_dir: Option<&'a str>,
    log_keep: Option<usize>,
    ui_port: Option<u16>,
    meters: Vec<(&'a str, &'a str, &'a str, bool, bool)>,
}

/// Project a stored configuration onto what the operator can set.
///
/// Exhaustive on purpose — no `..`. A new setting breaks this line, and its author
/// has to say whether editing it counts as changing the configuration.
fn settings(c: &StoredConfig) -> SettingsView<'_> {
    let StoredConfig {
        // Stamped by `save`, so comparing it would make every schema bump a
        // "change" — and it is not a setting anybody edits.
        schema_version: _,
        group_id,
        node_id,
        broker_host,
        broker_port,
        publish_period_secs,
        api_base,
        log_dir,
        log_keep,
        // A human act about an unchanged mapping, not a setting (story 5.3).
        mapping_confirmed: _,
        // The answers, not the question.
        created_ms: _,
        last_change_ms: _,
        ui_port,
        meters,
    } = c;
    SettingsView {
        group_id,
        node_id,
        broker_host,
        broker_port: *broker_port,
        publish_period_secs: *publish_period_secs,
        api_base: api_base.as_deref(),
        log_dir: log_dir.as_deref(),
        log_keep: *log_keep,
        ui_port: *ui_port,
        meters: meters
            .iter()
            .map(|m| {
                let StoredMeter {
                    meter_id,
                    device_id,
                    serial,
                    enabled,
                    priority,
                } = m;
                (
                    meter_id.as_str(),
                    device_id.as_str(),
                    serial.as_str(),
                    *enabled,
                    *priority,
                )
            })
            .collect(),
    }
}

/// Write the configuration atomically — temp file, `fsync`, `rename`,
/// `fsync(dir)`, all of it already in [`crate::persist::persist_atomic`].
///
/// **`mapping_confirmed` is computed here and the caller's value is discarded**
/// (Story 5.3 AC3). If this write changes what is published, the confirmation is
/// withdrawn; if it does not, whatever the stored file said is carried over.
///
/// That rule lives here rather than in the screen that saves, because a boolean
/// the UI is trusted to clear is a boolean that survives the one edit somebody
/// makes through a different path — a future API, a migration, a repair script.
/// This is the boundary every writer passes.
pub fn save(
    dir: &Path,
    config: &StoredConfig,
    now: crate::domain::UtcMillis,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut to_write = config.clone();
    // THE TWO DATES ARE COMPUTED HERE TOO, and the caller's are discarded — same
    // boundary, same argument as `mapping_confirmed` below ([ADR 0039]).
    //
    // `now` is passed rather than read: this module has no clock, and giving it one
    // would put a second time origin in the process (see `Control::clock`).
    let existing = read(dir).ok();
    to_write.created_ms = match &existing {
        // A readable file keeps whatever it had — including nothing, for a file
        // written before ADR 0039. It never acquires a creation date later, because
        // there is no true one to write.
        Some(stored) => stored.created_ms,
        // No file at all: this write IS the creation, and that is the one moment
        // the date can be known.
        None if !config_path(dir).exists() => Some(now.0),
        // A file exists and could not be read — a syntax error the form is
        // repairing. It may well have had a creation date; inventing one now would
        // claim this configuration was born at the moment somebody fixed a typo.
        None => None,
    };
    to_write.last_change_ms = match &existing {
        // A Save that changes nothing is not a change. Without this, "last change"
        // would mean "last time somebody pressed a button" — the distinction story
        // 6.3 drew between `last_changed_at` and `last_published_at`, one layer
        // down and for the same reader.
        Some(stored) if same_settings(stored, config) => stored.last_change_ms,
        _ => Some(now.0),
    };
    // THE CONSTANT IS STAMPED HERE, never taken from the caller.
    //
    // `save` wrote back whatever `schema_version` it was handed, so it was a
    // public path to persisting a file this build then refuses — and `main.rs`
    // exits on such a file before the UI is spawned, which makes it unrepairable
    // through the browser. Nothing in production passed a wrong version, but a
    // contract that holds only because every current caller is careful is the
    // kind this repository keeps paying for.
    to_write.schema_version = SCHEMA_VERSION;
    to_write.mapping_confirmed = match read(dir) {
        Ok(stored) if same_mapping(&stored, config) => stored.mapping_confirmed,
        // Unreadable, absent, or a different mapping — all three mean nobody has
        // confirmed what is about to be written.
        _ => false,
    };

    // A file whose contents this build cannot account for is not overwritten.
    //
    // `save` once consulted `read` and, on any error, cleared the confirmation and
    // wrote anyway — performing the very overwrite this module's own documentation
    // says must never happen ("lets a corrupt file be mistaken for a fresh install
    // and silently overwritten"). Only `phase::decide` upheld the rule; the writer
    // did not.
    //
    // The first repair of that went too far the other way: it refused on ANY `read`
    // error, and a syntax error is a `read` error — so the operator corrected the
    // form the `Misconfigured` screen had rendered for them, pressed Save, and was
    // told nothing had changed, for ever. See [`Overwrite`] for the line now drawn.
    if let Overwrite::WouldDestroy(why) = overwrite(dir) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, why));
    }
    persist::persist_atomic(&config_path(dir), &to_write)
}

/// Record that a human has looked at the mapping now on disk and said it is
/// right (FR25).
///
/// **Deliberately not routed through [`save`]**, and the asymmetry is the design
/// rather than a hole in it. `save` answers *"this mapping changed, so nobody
/// has confirmed it"*. This answers *"this mapping did not change, and somebody
/// just looked at it"* — it writes no setting, so there is nothing for the
/// withdrawal rule to act on. Sending it through `save` would clear the very
/// flag it exists to set.
pub fn confirm(dir: &Path) -> Result<StoredConfig, ConfigErrors> {
    let mut config = read(dir)?;
    config.mapping_confirmed = true;
    persist::persist_atomic(&config_path(dir), &config).map_err(|error| {
        ConfigErrors(vec![fault(
            "mapping confirmation",
            format!(
                "{} could not be written: {error}. The mapping stays unconfirmed, so the                  bridge publishes nothing — which is the safe direction, but it means the                  click did not take",
                config_path(dir).display()
            ),
        )])
    })?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "s3cr3t-do-not-print";

    fn dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("smartme_store_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        path
    }

    /// **[ADR 0040] on the path production actually takes** — added by the review
    /// of story 6.7, 2026-08-20.
    ///
    /// The migration was pinned on `read`, and `read` is what the SCREENS call. The
    /// bridge starts through `load`, and a migration that worked for the form while
    /// startup still refused would have produced the worst of both: a bridge that
    /// will not publish, showing a configuration screen that says everything is
    /// fine.
    ///
    /// `load` delegates to `read` today, so this passes as written — which is
    /// exactly why it is worth asserting: nothing stated that it must, and the two
    /// could be given separate version checks by anybody repairing one of them.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: giving `load` its own strict version
    /// check (`version != SCHEMA_VERSION` → refuse, before delegating) goes red with
    /// `the bridge must START on a migrated file`.
    #[test]
    fn the_bridge_starts_on_a_migrated_file_and_not_only_the_screens() {
        let home = dir("migrate_startup");
        let mut four = sound();
        four.schema_version = 4;
        std::fs::write(
            config_path(&home),
            toml::to_string(&four).expect("serialize"),
        )
        .expect("plant a version-4 file");

        let raw = load(&home, credential()).expect(
            "the bridge must START on a migrated file: a migration that only reached \
             the screens would leave a bridge refusing to publish while its own \
             configuration page rendered happily",
        );
        assert_eq!(raw.broker_host.as_deref(), Some("broker"));
        assert_eq!(raw.meters.len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// **A file that cannot be READ acquires no creation date** — the third arm of
    /// `save`'s rule, added by the review of story 6.7 because the story's own test
    /// covered the other two only.
    ///
    /// A syntax error the configuration form repairs is the live case ([ADR 0026]
    /// makes `Misconfigured` a startup state, and the form is where it is fixed).
    /// The file may well have had a creation date; stamping `now` would claim this
    /// configuration was born at the moment somebody corrected a typo, and carrying
    /// nothing forward is the only honest answer available.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: collapsing the unreadable arm into the
    /// no-file one (`None => Some(now.0)`) goes red with `a configuration repaired
    /// from a broken file was not created at the moment of the repair`.
    #[test]
    fn a_repaired_file_is_not_treated_as_a_new_configuration() {
        let home = dir("repaired_not_created");
        std::fs::write(config_path(&home), "this is not toml {{{").expect("plant a broken file");

        let now = crate::domain::UtcMillis(1_784_984_793_000);
        save(&home, &sound(), now).expect("the form repairs it");

        assert_eq!(
            read(&home).expect("reads now").created_ms,
            None,
            "a configuration repaired from a broken file was not created at the \
             moment of the repair: the file existed, its creation date may have \
             existed too, and neither is knowable now — so the honest answer is \
             that it is unknown"
        );
        assert_eq!(
            read(&home).expect("reads now").last_change_ms,
            Some(now.0),
            "but the CHANGE is knowable: this write changed the settings, whatever \
             they were before"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// **[ADR 0040] — a version-4 file reads, and every setting survives.**
    ///
    /// The bump from 4 to 5 is required by this module's own rule; without a
    /// migration it would cost the operator a full retype, because the version check
    /// is in `read` and `read` is what pre-fills the configuration screen. This test
    /// is the difference between "the version went up" and "the configuration
    /// survived it" — which is FR27.
    ///
    /// **The refusals are asserted in the same test**, and they are what keep the
    /// gate a gate: a file from the future, and one older than any migration
    /// written, are still refused. Without them this would pass against a build that
    /// simply stopped checking.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN, output copied: deleting the migration arm
    /// (every non-current version refused, the state before [ADR 0040]) goes red with
    ///
    /// ```text
    /// a version-4 file must READ: the version check is in `read`, so refusing it
    /// empties the very form the operator would repair it in
    ///   Err(ConfigErrors([Fault { field: "stored configuration", … "no migration
    ///   exists for that step" …}]))
    /// ```
    #[test]
    fn a_version_4_file_migrates_and_older_and_newer_ones_do_not() {
        let home = dir("migrate_4_to_5");

        // Written as TEXT, by hand: `save` stamps the constant, so it is
        // structurally incapable of producing the file under test — which is
        // written by ANOTHER BUILD, and that is the situation this test is about.
        let mut four = sound();
        four.schema_version = 4;
        four.created_ms = None;
        four.last_change_ms = None;
        std::fs::write(
            config_path(&home),
            toml::to_string(&four).expect("serialize"),
        )
        .expect("plant a version-4 file");

        let read_back = read(&home).expect(
            "a version-4 file must READ: the version check is in `read`, so refusing \
             it empties the very form the operator would repair it in",
        );
        assert_eq!(read_back.schema_version, SCHEMA_VERSION);
        assert_eq!(
            read_back.broker_host, four.broker_host,
            "and every setting survives the step, or the migration is a data loss \
             wearing a version number"
        );
        assert_eq!(read_back.meters.len(), four.meters.len());
        assert_eq!(read_back.meters[0].serial, four.meters[0].serial);
        assert!(
            read_back.mapping_confirmed,
            "the confirmation survives too: nothing about the mapping changed"
        );
        assert_eq!(
            (read_back.created_ms, read_back.last_change_ms),
            (None, None),
            "and the two new dates are ABSENT rather than invented — the file was \
             written before anything recorded them ([ADR 0039])"
        );
        assert!(
            !read_back.meters[0].priority,
            "and nothing is marked as mattering, because nobody marked it"
        );

        // A file from the future stays refused: this build cannot know what it says.
        let mut future = sound();
        future.schema_version = SCHEMA_VERSION + 1;
        std::fs::write(
            config_path(&home),
            toml::to_string(&future).expect("serialize"),
        )
        .expect("write");
        let errors = read(&home).expect_err("a future schema must be refused");
        assert!(
            format!("{errors}").contains("no migration exists for that step"),
            "and the refusal must say why it is not read anyway: {errors}"
        );

        // So does one older than any migration written.
        let mut ancient = sound();
        ancient.schema_version = 2;
        std::fs::write(
            config_path(&home),
            toml::to_string(&ancient).expect("serialize"),
        )
        .expect("write");
        read(&home).expect_err(
            "a version below the oldest migration is refused: writing one for a file \
             nobody has would be code with no evidence behind it",
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    /// **Story 6.7 AC2 — the creation date is stamped once, by the writer, and
    /// only when there was nothing to carry over.**
    ///
    /// The three cases are one test because the rule is one rule and its edges are
    /// where it goes wrong: a first write knows it is the creation, a later write
    /// must not re-stamp it, and a file that predates [ADR 0039] must never acquire
    /// one — the only honest answer about a configuration whose birth nobody
    /// recorded is that it is unknown.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN, output copied: stamping unconditionally
    /// (`to_write.created_ms = Some(now.0)`, the obvious one-liner) goes red with
    ///
    /// ```text
    /// a later write must not restamp the creation: this configuration was created
    /// once, and every Save would move the date the screen shows
    ///   left: Some(1784984793000)
    ///  right: Some(1784984000000)
    /// ```
    #[test]
    fn the_creation_date_is_stamped_once_and_never_invented() {
        let home = dir("created_once");
        let first = crate::domain::UtcMillis(1_784_984_000_000);
        let later = crate::domain::UtcMillis(1_784_984_793_000);

        save(&home, &sound(), first).expect("the first write");
        assert_eq!(
            read(&home).expect("reads").created_ms,
            Some(first.0),
            "a write with no file to carry over from IS the creation, and it is the \
             one moment the date can be known"
        );

        let mut edited = sound();
        edited.broker_port = 8883;
        save(&home, &edited, later).expect("the edit");
        assert_eq!(
            read(&home).expect("reads").created_ms,
            Some(first.0),
            "a later write must not restamp the creation: this configuration was \
             created once, and every Save would move the date the screen shows"
        );

        // A file from before ADR 0039: it reads, and it has no creation date.
        let old = dir("created_never");
        let mut without = sound();
        without.created_ms = None;
        crate::persist::persist_atomic(&config_path(&old), &without).expect("plant it");
        save(&old, &sound(), later).expect("write over it");
        assert_eq!(
            read(&old).expect("reads").created_ms,
            None,
            "and a configuration whose birth nobody recorded stays unknown: a date \
             invented at the moment somebody edited it would be a plausible lie, \
             which is the one thing this bridge exists not to produce"
        );
    }

    /// **Story 6.7 AC3 — a Save that changes nothing is not a change.**
    ///
    /// Without this, "last change" comes to mean "last time somebody pressed a
    /// button", and the line on the state screen stops answering the question it
    /// exists for. It is the same distinction story 6.3 drew between
    /// `last_changed_at` and `last_published_at`, one layer down.
    ///
    /// **The confirmation is asserted too**, because it shares the comparison's
    /// neighbourhood: a date that moved while `mapping_confirmed` survived would
    /// mean the two comparisons had drifted apart.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: stamping `last_change_ms` on every write
    /// goes red with `a Save that changed nothing must not move the last change …
    /// left: Some(1784984793000), right: Some(1784984000000)`.
    #[test]
    fn a_save_that_changes_nothing_does_not_move_the_last_change() {
        let home = dir("unchanged_save");
        let first = crate::domain::UtcMillis(1_784_984_000_000);
        let later = crate::domain::UtcMillis(1_784_984_793_000);

        save(&home, &sound(), first).expect("the first write");
        let after_first = read(&home).expect("reads");
        assert_eq!(after_first.last_change_ms, Some(first.0));

        save(&home, &sound(), later).expect("the same settings again");
        let after_second = read(&home).expect("reads");
        assert_eq!(
            after_second.last_change_ms,
            Some(first.0),
            "a Save that changed nothing must not move the last change: the line on \
             the state screen would then report when somebody last pressed a button"
        );

        // And a real edit does move it — without this half the assertion above
        // would pass against a field nothing ever writes.
        let mut edited = sound();
        edited.publish_period_secs = 60;
        save(&home, &edited, later).expect("a real edit");
        assert_eq!(
            read(&home).expect("reads").last_change_ms,
            Some(later.0),
            "and a settings change moves it, or the field is decorative"
        );
    }

    /// **Story 6.7 AC1 — marking a meter as one that matters is not a mapping
    /// change.**
    ///
    /// `priority` says nothing about what is published for that meter, so it must
    /// not withdraw a confirmation the operator has already given. It IS a settings
    /// change, so `last_change_ms` moves. The two comparisons answer two different
    /// questions, and this pins that they do.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: adding `priority` to
    /// `mapping_projection` goes red with `marking a meter as one that matters must
    /// not cost a confirmation click`.
    #[test]
    fn priority_moves_the_change_date_and_not_the_confirmation() {
        let home = dir("priority_not_mapping");
        let first = crate::domain::UtcMillis(1_784_984_000_000);
        let later = crate::domain::UtcMillis(1_784_984_793_000);

        save(&home, &sound(), first).expect("write");
        confirm(&home).expect("a human looks at the mapping and says it is right");

        let mut starred = sound();
        starred.meters[0].priority = true;
        save(&home, &starred, later).expect("mark it");
        let after = read(&home).expect("reads");
        assert!(
            after.mapping_confirmed,
            "marking a meter as one that matters must not cost a confirmation \
             click: nothing about what is published for it changed"
        );
        assert_eq!(
            after.last_change_ms,
            Some(later.0),
            "and it IS a change to the configuration, so the date says so"
        );
        assert!(
            after.meters[0].priority,
            "and the mark itself survives the write"
        );
    }

    fn sound() -> StoredConfig {
        StoredConfig {
            created_ms: None,
            last_change_ms: None,
            schema_version: SCHEMA_VERSION,
            group_id: "Group".into(),
            node_id: "Node".into(),
            broker_host: "broker".into(),
            broker_port: 1883,
            publish_period_secs: 30,
            api_base: None,
            log_dir: None,
            log_keep: None,
            mapping_confirmed: true,
            ui_port: None,
            meters: vec![StoredMeter {
                priority: false,
                meter_id: "meter-a".into(),
                device_id: "dev-a".into(),
                serial: "9202685".into(),
                enabled: true,
            }],
        }
    }

    fn credential() -> Credential {
        Credential {
            client_id: Some("id".into()),
            client_secret: Some(SECRET.into()),
        }
    }

    /// **Story 3.4 AC5, [#64] — membership in the mapping is decided in one
    /// place, and this test walks the decision field by field.**
    ///
    /// Both halves matter equally. A NOT-MAPPING field moving the projection
    /// would withdraw confirmations on changes that have nothing to do with the
    /// mapping — the "different defect wearing the same fix" the issue warned
    /// a mechanical derive would produce. A MAPPING field failing to move it is
    /// [#64]'s original hazard: a save that keeps a confirmation no human gave
    /// to the new mapping.
    #[test]
    fn the_projection_decides_membership_for_every_field() {
        let base = {
            let mut c = sound();
            // Two rows, so reordering has something to reorder.
            c.meters.push(StoredMeter {
                priority: false,
                meter_id: "meter-b".into(),
                device_id: "dev-b".into(),
                serial: "6387987".into(),
                enabled: false,
            });
            c
        };

        type Edit = fn(&mut StoredConfig);
        let not_mapping: [(&str, Edit); 9] = [
            ("schema_version", |c| c.schema_version += 1),
            ("broker_host", |c| c.broker_host = "elsewhere".into()),
            ("broker_port", |c| c.broker_port = 8883),
            ("publish_period_secs", |c| c.publish_period_secs = 60),
            ("api_base", |c| {
                c.api_base = Some("https://mirror.example".into());
            }),
            ("log_dir", |c| c.log_dir = Some("/logs".into())),
            ("log_keep", |c| c.log_keep = Some(3)),
            ("mapping_confirmed", |c| c.mapping_confirmed = false),
            ("ui_port", |c| c.ui_port = Some(9090)),
        ];
        for (field, mutate) in not_mapping {
            let mut edited = base.clone();
            mutate(&mut edited);
            assert!(
                same_mapping(&base, &edited),
                "{field} is not part of the mapping: changing it must not \
                 withdraw a confirmation given for an unchanged meter→topic \
                 attribution"
            );
        }
        // Row order is not membership either — both readers sort.
        let mut reordered = base.clone();
        reordered.meters.reverse();
        assert!(
            same_mapping(&base, &reordered),
            "reordering rows changes nothing about what reaches the wire"
        );

        let mapping: [(&str, Edit); 8] = [
            ("group_id", |c| c.group_id = "Other".into()),
            ("node_id", |c| c.node_id = "other-node".into()),
            ("meters[].meter_id", |c| {
                c.meters[0].meter_id = "renamed".into();
            }),
            ("meters[].device_id", |c| {
                c.meters[0].device_id = "dev-z".into();
            }),
            ("meters[].serial", |c| c.meters[0].serial = "1111111".into()),
            // The DISABLED row: editing it changes nothing on the wire today
            // and still deserves a fresh look — the position `same_mapping`
            // has always held.
            ("a disabled meter's field", |c| {
                c.meters[1].serial = "2222222".into();
            }),
            ("meters[].enabled", |c| c.meters[1].enabled = true),
            ("a row added", |c| c.meters.push(c.meters[0].clone())),
        ];
        for (field, mutate) in mapping {
            let mut edited = base.clone();
            mutate(&mut edited);
            assert!(
                !same_mapping(&base, &edited),
                "{field} IS the mapping: a human vouched for something this \
                 change replaced, and the confirmation must be withdrawn"
            );
            assert_ne!(
                mapping_projection(&base).canonical(),
                mapping_projection(&edited).canonical(),
                "{field}: and the fingerprint's bytes must move with it — one \
                 projection, two readers, one answer"
            );
        }
    }

    /// **Review repair — the canonical form is injective.** The separators are
    /// legal inside every field they delimit, so before the length prefixes two
    /// mappings that `same_mapping` calls DIFFERENT could hash identically —
    /// and the fingerprint exists precisely to catch an interleaved write.
    #[test]
    fn the_canonical_form_is_injective_where_the_separators_are_hostile() {
        // The field boundary: ("a\u{1f}b", "") vs ("a", "b").
        let mut left = sound();
        left.meters[0].meter_id = "a\u{1f}b".into();
        left.meters[0].device_id = String::new();
        let mut right = sound();
        right.meters[0].meter_id = "a".into();
        right.meters[0].device_id = "b".into();
        assert!(!same_mapping(&left, &right), "the premise: they differ");
        assert_ne!(
            mapping_projection(&left).canonical(),
            mapping_projection(&right).canonical(),
            "a separator inside a field must not let two different mappings \
             hash to one fingerprint — that is a forgeable guard"
        );

        // The record boundary: group "A\u{1e}B" / node "C" vs "A" / "B\u{1e}C".
        let mut left = sound();
        left.group_id = "A\u{1e}B".into();
        left.node_id = "C".into();
        let mut right = sound();
        right.group_id = "A".into();
        right.node_id = "B\u{1e}C".into();
        assert!(!same_mapping(&left, &right), "the premise: they differ");
        assert_ne!(
            mapping_projection(&left).canonical(),
            mapping_projection(&right).canonical(),
            "a group id must not be able to impersonate a node boundary"
        );

        // The ROW boundary — the case the original finding named and the first
        // version of this test skipped (found by the review of the repair): a
        // field carrying `\u{1e}` must not read as one row ending and another
        // beginning.
        let mut left = sound();
        left.meters[0].meter_id =
            format!("m\u{1f}d\u{1f}s\u{1f}true\u{1e}{}", left.meters[0].meter_id);
        let mut right = sound();
        right.meters.push(StoredMeter {
            priority: false,
            meter_id: "m".into(),
            device_id: "d".into(),
            serial: "s".into(),
            enabled: true,
        });
        assert!(!same_mapping(&left, &right), "the premise: they differ");
        assert_ne!(
            mapping_projection(&left).canonical(),
            mapping_projection(&right).canonical(),
            "a meter field must not be able to impersonate a row boundary — a \
             future refactor dropping one length prefix would fail here"
        );
    }

    #[test]
    fn a_saved_configuration_round_trips_through_the_same_validation_as_the_environment() {
        let dir = dir("roundtrip");
        save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        let raw = load(&dir, credential()).expect("load");
        let validated = crate::app::config::validate(raw).expect("validates");
        assert_eq!(validated.meters.len(), 1);
        assert_eq!(validated.poll.interval.as_secs(), 30);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC2 — the seam. `exists` is what separates "first run" from "refuse to
    /// start", and merging those two is how a first run gets bricked.
    ///
    /// FALSIFIED 2026-08-04 by mutating `exists` to always return `false`, which
    /// is the collapse that reads a real configuration as a fresh install:
    ///
    /// ```text
    /// test a_directory_without_a_config_file_is_absence_not_a_fault ... FAILED
    /// and a written one does
    /// ```
    #[test]
    fn a_directory_without_a_config_file_is_absence_not_a_fault() {
        let dir = dir("absent");
        assert!(!exists(&dir), "an empty directory holds no configuration");
        save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        assert!(exists(&dir), "and a written one does");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC5 — the trap is that serde would have made this SUCCEED.
    #[test]
    fn a_file_from_another_schema_version_is_refused_rather_than_defaulted() {
        let dir = dir("schema");
        // Written as TEXT, by hand — never through `save`.
        //
        // `save` stamps the constant (since 2026-08-05), which is what stops a
        // caller persisting a file this build refuses. That makes it structurally
        // incapable of producing the file under test here, and it should be: a
        // foreign-schema file is written by ANOTHER BUILD, which is the situation
        // this test is about. The previous version wrote it through `save` and
        // was therefore coupled to the defect — stamping the constant would have
        // "broken" it.
        let mut config = sound();
        config.schema_version = SCHEMA_VERSION + 1;
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            config_path(&dir),
            toml::to_string(&config).expect("serialize"),
        )
        .expect("write");
        let errors = load(&dir, credential()).expect_err("a future schema must be refused");
        assert!(
            format!("{errors}").contains("no migration"),
            "the refusal must say why it is not read anyway: {errors}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An unknown field means a rename this build does not understand. Ignoring
    /// it — serde's default — starts the bridge on a value nobody wrote.
    ///
    /// FALSIFIED 2026-08-03, and the first attempt **proved nothing**. The
    /// unknown field was appended to the end of the file, which in TOML puts it
    /// inside the last `[[meters]]` table rather than at the root — so removing
    /// `deny_unknown_fields` from `StoredConfig` left the test green, refused by
    /// `StoredMeter` instead. Right outcome, wrong reason, exactly the shape this
    /// project keeps catching. The line is now inserted at the root, before the
    /// meters array, and the mutation is red:
    ///
    /// ```text
    /// an_unknown_field_is_refused_rather_than_ignored ... FAILED
    /// an unknown field must be refused: ...
    /// ```
    ///
    /// Both structs are covered, deliberately: the second half of the test puts
    /// one inside a meter as well.
    /// FR25 reached through the path the manual recommends: confirm, then correct
    /// the node id, then restart.
    ///
    /// Both identifiers are exercised, and separately — a rule that caught
    /// `group_id` and missed `node_id` would pass a test that only tried one.
    ///
    /// FALSIFIED 2026-08-06 by restoring the meters-only comparison
    /// (`same_mapping(&stored.meters, &config.meters)` over the old signature).
    /// Copied from the run:
    ///
    /// ```text
    /// test app::store::tests::changing_the_node_identity_withdraws_the_confirmation ... FAILED
    ///
    /// thread '…changing_the_node_identity_withdraws_the_confirmation' (353) panicked at
    /// crates/smartme-bridge/src/app/store.rs:730:13:
    /// node_id is in every topic the bridge publishes, so changing it changes where every
    /// device lands: the confirmation must not survive it
    ///
    /// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 164 filtered out
    /// ```
    ///
    /// It dies on the `node_id` iteration, which is the first — so the `group_id`
    /// half is proved by the same mutation only in that it shares the rule, not by
    /// having been reached. Both are asserted; only one is falsified.
    #[test]
    fn changing_the_node_identity_withdraws_the_confirmation() {
        for (label, mutate) in [
            (
                "node_id",
                (|c: &mut StoredConfig| c.node_id = "Bridge02".into()) as fn(&mut StoredConfig),
            ),
            ("group_id", |c: &mut StoredConfig| {
                c.group_id = "OtherPlant".into()
            }),
        ] {
            let dir = dir(&format!("identity_{label}"));
            let _ = std::fs::remove_dir_all(&dir);
            let confirmed = sound();
            assert!(
                confirmed.mapping_confirmed,
                "the premise: the stored file must be confirmed, or this proves nothing"
            );
            save(
                &dir,
                &confirmed,
                crate::domain::UtcMillis(1_784_984_793_000),
            )
            .expect("the first write");
            confirm(&dir).expect("confirm");

            let mut edited = sound();
            mutate(&mut edited);
            save(&dir, &edited, crate::domain::UtcMillis(1_784_984_793_000)).expect("the edit");

            let back = read(&dir).expect("read back");
            assert!(
                !back.mapping_confirmed,
                "{label} is in every topic the bridge publishes, so changing it changes \
                 where every device lands: the confirmation must not survive it"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// The lock-out the second review round found: the screen that exists to
    /// repair a broken file could not write the repair.
    ///
    /// The assertion ends at `read`, not at `save`'s `Ok`. A `save` that returned
    /// `Ok(())` and wrote nothing — or wrote an empty document — would satisfy
    /// "the repair was accepted" while leaving the operator exactly as stuck, so
    /// the test demands the corrected settings back out of the file.
    ///
    /// FALSIFIED 2026-08-06 by restoring the guard this replaces
    /// (`if exists(dir) && read(dir).is_err()`). Copied from the run, and note it
    /// dies on the `save` under repair — not on the premise above it:
    ///
    /// ```text
    /// test app::store::tests::a_syntactically_broken_file_can_be_repaired_through_save ... FAILED
    ///
    /// thread '…a_syntactically_broken_file_can_be_repaired_through_save' (353) panicked at
    /// crates/smartme-bridge/src/app/store.rs:706:13:
    /// the Misconfigured screen renders this file into a form; the form must be able to write
    /// it back: Custom { kind: InvalidData, error: "/tmp/…/smartme_store_352_repairable/config.toml
    /// exists and cannot be read, so it was NOT overwritten. Fix or remove it; refusing beats
    /// destroying a configuration that may only be unreadable by this build" }
    ///
    /// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 163 filtered out
    /// ```
    #[test]
    fn a_syntactically_broken_file_can_be_repaired_through_save() {
        let dir = dir("repairable");
        std::fs::create_dir_all(&dir).expect("dir");
        // Unterminated string: `read` fails, and this is precisely the shape a
        // hand-edited file takes when FR23's headless bring-up goes wrong.
        std::fs::write(config_path(&dir), "schema_version = 4\ngroup_id = \"Grou\n")
            .expect("write");
        read(&dir).expect_err("the premise: this file must be unreadable, or the test is vacuous");

        let repaired = StoredConfig {
            group_id: "Repaired".into(),
            ..sound()
        };
        save(&dir, &repaired, crate::domain::UtcMillis(1_784_984_793_000)).unwrap_or_else(|e| {
            panic!(
                "the Misconfigured screen renders this file into a form; the form must \
                 be able to write it back: {e:?}"
            )
        });

        let back = read(&dir).expect("the repaired file must now read");
        assert_eq!(back.group_id, "Repaired");
        assert_eq!(back.broker_host, "broker");
        assert_eq!(
            back.schema_version, SCHEMA_VERSION,
            "the repair must carry this build's version, not the broken file's"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The half that must keep refusing, and the reason the guard existed at all:
    /// a file from a NEWER image holds fields this build cannot represent, so
    /// writing over it destroys settings rather than repairing them.
    ///
    /// The assertion names the version, not merely "an error". `save` has three
    /// other ways to fail (`create_dir_all`, the serializer, `persist_atomic`),
    /// and `is_err()` would be satisfied by any of them.
    ///
    /// FALSIFIED 2026-08-06 by weakening the comparison in [`overwrite`] to
    /// `schema_version == 0`, so the newer file is treated as repairable. Copied
    /// from the run — it dies on the `expect_err`, which is the line under repair:
    ///
    /// ```text
    /// test app::store::tests::a_file_from_a_newer_image_is_still_not_overwritten ... FAILED
    ///
    /// thread '…a_file_from_a_newer_image_is_still_not_overwritten' (353) panicked at
    /// crates/smartme-bridge/src/app/store.rs:743:42:
    /// a newer image's file must be protected, and the refusal must name the version: ()
    ///
    /// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 163 filtered out
    /// ```
    ///
    /// (`()` and not `Ok(())`: `expect_err` prints the `Ok` value, and `save`
    /// returns `io::Result<()>`. The first draft of this record said `Ok(())`,
    /// which is how a record that is written rather than copied reads.)
    #[test]
    fn a_file_from_a_newer_image_is_still_not_overwritten() {
        let dir = dir("from_the_future");
        std::fs::create_dir_all(&dir).expect("dir");
        // Valid TOML, a version beyond this build, and a field it has never heard
        // of — which is why the probe must not use `deny_unknown_fields`.
        let future = format!(
            "schema_version = {}\ngroup_id = \"G\"\nnode_id = \"N\"\nbroker_host = \"b\"\n\
             broker_port = 1883\npublish_period_secs = 30\nsomething_new = \"kept\"\nmeters = []\n",
            SCHEMA_VERSION + 1
        );
        std::fs::write(config_path(&dir), &future).expect("write");

        let error = save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect_err(
            "a newer image's file must be protected, and the refusal must name the version",
        );
        let said = error.to_string();
        assert!(
            said.contains(&format!("schema version {}", SCHEMA_VERSION + 1)),
            "the refusal must name the version that owns the file, so the operator \
             knows to roll the image forward rather than delete it; got: {said}"
        );

        let on_disk = std::fs::read_to_string(config_path(&dir)).expect("read back");
        assert_eq!(
            on_disk, future,
            "refusing is only half of it — the file must be byte-for-byte untouched"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let root_dir = dir("unknown");
        let meter_dir = dir("unknown_meter");
        let dir = root_dir;
        let config = sound();
        std::fs::create_dir_all(&dir).expect("dir");
        let text = toml::to_string(&config).expect("serialize");
        // At the ROOT — before any table header, or TOML makes it a member of the
        // last table and this asserts something else entirely.
        std::fs::write(
            config_path(&dir),
            format!("publish_period_seconds = 300\n{text}"),
        )
        .expect("write");
        let errors = load(&dir, credential()).expect_err("an unknown ROOT field must be refused");
        assert!(
            errors.0.iter().any(|f| f.field == "stored configuration"),
            "got {:?}",
            errors.0
        );

        // And inside a meter, which is a different struct and a different derive.
        //
        // A FRESH directory: `save` now refuses to overwrite a file that exists
        // and cannot be read, which is the point of the previous half. Reusing
        // this one would test the refusal instead of the derive.
        let mut text = toml::to_string(&config).expect("serialize");
        text.push_str("\nserial_number = \"9202685\"\n");
        std::fs::create_dir_all(&meter_dir).expect("dir");
        std::fs::write(config_path(&meter_dir), text).expect("write");
        let errors =
            load(&meter_dir, credential()).expect_err("an unknown METER field must be refused");
        assert!(
            errors.0.iter().any(|f| f.field == "stored configuration"),
            "got {:?}",
            errors.0
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&meter_dir);
    }

    /// AC6, and the strongest form of it: not *redacted* on the way to disk, but
    /// never on that path at all.
    ///
    /// The absence assertion is guarded. Asserting "the file does not contain the
    /// secret" over a file that was never written, or written empty, holds
    /// trivially — this project has already shipped absence assertions that held
    /// over an empty stream. So the file is first proved to carry the settings
    /// that ARE stored, and only then proved not to carry the one that is not.
    ///
    /// FALSIFIED 2026-08-04 by mutating `save` to write an empty document,
    /// and **the guard is what fired, not the absence** — which is the whole
    /// demonstration:
    ///
    /// ```text
    /// test the_secret_is_absent_from_the_file_because_it_never_had_a_path_to_it ... FAILED
    /// the absence assertion below needs a file that actually carries settings:
    /// ```
    ///
    /// Had the guard not been there, an empty file would have passed this test
    /// while proving nothing whatsoever about where the secret goes.
    #[test]
    fn the_secret_is_absent_from_the_file_because_it_never_had_a_path_to_it() {
        let dir = dir("no-secret-on-disk");
        save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        let written = std::fs::read_to_string(config_path(&dir)).expect("read");

        assert!(
            written.contains("9202685") && written.contains("broker"),
            "the absence assertion below needs a file that actually carries settings: {written}"
        );
        assert!(
            !written.contains(SECRET) && !written.contains("client_secret"),
            "the credential reached the disk: {written}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Story 5.3 AC3 — **the withdrawal is structural.** A caller that changes
    /// the mapping and asserts confirmation in the same write does not get to.
    ///
    /// FALSIFIED 2026-08-04 by making `save` write what it is given:
    ///
    /// ```text
    /// test changing_the_mapping_withdraws_the_confirmation_whatever_the_caller_says ... FAILED
    /// a mapping nobody has looked at was recorded as confirmed
    /// ```
    ///
    /// The two neighbouring tests stayed GREEN under that mutation, which is
    /// what makes this one worth having on its own: preserving a confirmation
    /// across an unrelated write is easy, and it is not the property at issue.
    #[test]
    fn changing_the_mapping_withdraws_the_confirmation_whatever_the_caller_says() {
        let dir = dir("withdraw");
        save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        confirm(&dir).expect("a human confirms it");
        assert!(read(&dir).expect("read").mapping_confirmed);

        // A different serial IS a different device on the wire — and the caller
        // insists it is confirmed, which is exactly the write the rule exists
        // for. `sound()` sets the flag true, so this is not a straw man.
        let mut moved = sound();
        moved.meters[0].serial = "9202699".into();
        assert!(
            moved.mapping_confirmed,
            "the fixture must assert it, or this proves nothing"
        );
        save(&dir, &moved, crate::domain::UtcMillis(1_784_984_793_000)).expect("save");

        assert!(
            !read(&dir).expect("read").mapping_confirmed,
            "a mapping nobody has looked at was recorded as confirmed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A duplicated meter must not turn the comparison into a subset test.
    ///
    /// FOUND BY REVIEW 2026-08-04, against code that had been falsified three
    /// times — none of the three mutations targeted `same_mapping`, which is why
    /// it survived. `[M, M]` and `[M, N]` have equal length and every element of
    /// the first appears in the second, so a subset test called them the same
    /// mapping and carried the confirmation across.
    #[test]
    fn a_duplicated_meter_does_not_carry_a_confirmation_across_a_changed_mapping() {
        let dir = dir("dup");
        let mut doubled = sound();
        doubled.meters = vec![doubled.meters[0].clone(), doubled.meters[0].clone()];
        save(&dir, &doubled, crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        confirm(&dir).expect("confirm the doubled mapping");

        let mut changed = sound();
        changed.meters = vec![
            doubled.meters[0].clone(),
            StoredMeter {
                priority: false,
                meter_id: "cellar".into(),
                device_id: "dev-new".into(),
                serial: "9209999".into(),
                enabled: true,
            },
        ];
        save(&dir, &changed, crate::domain::UtcMillis(1_784_984_793_000)).expect("save");

        assert!(
            !read(&dir).expect("read").mapping_confirmed,
            "a device nobody has ever seen was about to be born under a \
             confirmation given for a different mapping"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other half: a write that does NOT touch the mapping must not cost the
    /// operator a second click. A guard that lapses for no visible reason is a
    /// guard that gets switched off.
    #[test]
    fn changing_something_that_is_not_the_mapping_keeps_the_confirmation() {
        let dir = dir("keep");
        save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        confirm(&dir).expect("confirm");

        let mut faster = sound();
        faster.publish_period_secs = 5;
        save(&dir, &faster, crate::domain::UtcMillis(1_784_984_793_000)).expect("save");

        let back = read(&dir).expect("read");
        assert!(back.mapping_confirmed, "the period is not the mapping");
        assert_eq!(back.publish_period_secs, 5, "and the period did change");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Reordering is not a change — same rule as `reconfigure::classify`.
    #[test]
    fn reordering_the_meters_does_not_withdraw_the_confirmation() {
        let dir = dir("reorder");
        let mut two = sound();
        two.meters.push(StoredMeter {
            priority: false,
            meter_id: "cellar".into(),
            device_id: "dev-b".into(),
            serial: "9202686".into(),
            enabled: false,
        });
        save(&dir, &two, crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        confirm(&dir).expect("confirm");

        two.meters.reverse();
        save(&dir, &two, crate::domain::UtcMillis(1_784_984_793_000)).expect("save");

        assert!(
            read(&dir).expect("read").mapping_confirmed,
            "sorting a list in a form publishes nothing different, so it must not \
             cost a confirmation"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC2 — a file written by hand may assert the confirmation, because writing
    /// the mapping out IS having looked at it. This is the headless bring-up
    /// FR23 promises, and it must not require computing anything.
    #[test]
    fn a_hand_written_file_may_confirm_itself() {
        let dir = dir("hand");
        std::fs::write(
            config_path(&dir),
            format!(
                "schema_version = {SCHEMA_VERSION}\n\
                 group_id = \"G\"\nnode_id = \"N\"\n\
                 broker_host = \"b\"\nbroker_port = 1883\n\
                 publish_period_secs = 30\n\
                 mapping_confirmed = true\n\
                 \n[[meters]]\n\
                 meter_id = \"garage\"\ndevice_id = \"d\"\n\
                 serial = \"9202685\"\nenabled = true\n"
            ),
        )
        .expect("write");
        assert!(read(&dir).expect("read").mapping_confirmed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// And the default is the safe direction: a file that never heard of the key
    /// reads as UNCONFIRMED, which costs one click.
    ///
    /// FALSIFIED 2026-08-04 by flipping the serde default to `true`:
    ///
    /// ```text
    /// test a_file_without_the_key_is_unconfirmed_not_confirmed ... FAILED
    /// defaulting the other way would publish a mapping nobody had looked at
    /// ```
    #[test]
    fn a_file_without_the_key_is_unconfirmed_not_confirmed() {
        let dir = dir("absent-key");
        std::fs::write(
            config_path(&dir),
            format!(
                "schema_version = {SCHEMA_VERSION}\n\
                 group_id = \"G\"\nnode_id = \"N\"\n\
                 broker_host = \"b\"\nbroker_port = 1883\n\
                 publish_period_secs = 30\n\
                 \n[[meters]]\n\
                 meter_id = \"garage\"\ndevice_id = \"d\"\n\
                 serial = \"9202685\"\nenabled = true\n"
            ),
        )
        .expect("write");
        assert!(
            !read(&dir).expect("read").mapping_confirmed,
            "defaulting the other way would publish a mapping nobody had looked at"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC6 — `Debug` is hand-written on `Credential` for this reason, and a
    /// derive would defeat it silently.
    ///
    /// FALSIFIED 2026-08-04 by replacing the hand-written impl with
    /// `#[derive(Debug)]`:
    ///
    /// ```text
    /// test the_credential_debug_renders_without_the_secret ... FAILED
    /// a derived Debug would have leaked it: Credential { client_id: Some("id"),
    /// client_secret: Some("s3cr3t-do-not-print") }
    /// ```
    #[test]
    fn the_credential_debug_renders_without_the_secret() {
        let debugged = format!("{:?}", credential());
        assert!(
            !debugged.contains(SECRET),
            "a derived Debug would have leaked it: {debugged}"
        );
        assert!(
            debugged.contains("<redacted>"),
            "the absence assertion above needs the struct to have rendered at all: {debugged}"
        );
    }
}
