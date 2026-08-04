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
//! the period silently reverted. Refusing is the only honest answer until a
//! migration exists to be the other one.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
//! [ADR 0022]: ../../../docs/adr/0022-secrets-rest-in-a-separate-0600-file.md

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::config::{ConfigErrors, Fault, RawConfig, RawMeter};
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
pub const SCHEMA_VERSION: u32 = 4;

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
/// Present-but-invalid means a refusal to start (Story 5.1). Collapsing them
/// either bricks the first run or lets a corrupt file be mistaken for a fresh
/// install and silently overwritten.
pub fn exists(dir: &Path) -> bool {
    config_path(dir).exists()
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
        Err(error) => {
            faults.push(fault(
                "stored configuration",
                format!(
                    "{} could not be read: {error}. Refusing to start beats starting on \
                     defaults nobody chose",
                    config_path(dir).display()
                ),
            ));
            None
        }
    };

    if let Some(version) = config.as_ref().map(|c| c.schema_version)
        && version != SCHEMA_VERSION
    {
        faults.push(fault(
            "stored configuration",
            format!(
                "was written by schema version {version}, this build reads {SCHEMA_VERSION}. \
                 There is no migration, so it is refused rather than read by guesswork — an \
                 unrecognised field would otherwise take its default and the bridge would run \
                 on settings nobody wrote"
            ),
        ));
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
        log_keep: config.log_keep,
        ui_port: config.ui_port,
        meters: config
            .meters
            .into_iter()
            .map(|m| RawMeter {
                meter_id: Some(m.meter_id),
                device_id: Some(m.device_id),
                serial: Some(m.serial),
                enabled: Some(m.enabled),
            })
            .collect(),
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
fn same_mapping(a: &[StoredMeter], b: &[StoredMeter]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    // Multiset equality by counting, which needs no `Ord` on `StoredMeter`.
    a.iter()
        .all(|m| a.iter().filter(|x| *x == m).count() == b.iter().filter(|x| *x == m).count())
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
pub fn save(dir: &Path, config: &StoredConfig) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut to_write = config.clone();
    to_write.mapping_confirmed = match read(dir) {
        Ok(stored) if same_mapping(&stored.meters, &config.meters) => stored.mapping_confirmed,
        // Unreadable, absent, or a different mapping — all three mean nobody has
        // confirmed what is about to be written.
        _ => false,
    };
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

    fn sound() -> StoredConfig {
        StoredConfig {
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

    #[test]
    fn a_saved_configuration_round_trips_through_the_same_validation_as_the_environment() {
        let dir = dir("roundtrip");
        save(&dir, &sound()).expect("save");
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
        save(&dir, &sound()).expect("save");
        assert!(exists(&dir), "and a written one does");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC5 — the trap is that serde would have made this SUCCEED.
    #[test]
    fn a_file_from_another_schema_version_is_refused_rather_than_defaulted() {
        let dir = dir("schema");
        let mut config = sound();
        config.schema_version = SCHEMA_VERSION + 1;
        save(&dir, &config).expect("save");
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
    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let dir = dir("unknown");
        let config = sound();
        save(&dir, &config).expect("save");

        // At the ROOT — before any table header, or TOML makes it a member of the
        // last table and this asserts something else entirely.
        let text = std::fs::read_to_string(config_path(&dir)).expect("read");
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
        save(&dir, &config).expect("save");
        let mut text = std::fs::read_to_string(config_path(&dir)).expect("read");
        text.push_str("\nserial_number = \"9202685\"\n");
        std::fs::write(config_path(&dir), text).expect("write");
        let errors = load(&dir, credential()).expect_err("an unknown METER field must be refused");
        assert!(
            errors.0.iter().any(|f| f.field == "stored configuration"),
            "got {:?}",
            errors.0
        );

        let _ = std::fs::remove_dir_all(&dir);
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
        save(&dir, &sound()).expect("save");
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
        save(&dir, &sound()).expect("save");
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
        save(&dir, &moved).expect("save");

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
        save(&dir, &doubled).expect("save");
        confirm(&dir).expect("confirm the doubled mapping");

        let mut changed = sound();
        changed.meters = vec![
            doubled.meters[0].clone(),
            StoredMeter {
                meter_id: "cellar".into(),
                device_id: "dev-new".into(),
                serial: "9209999".into(),
                enabled: true,
            },
        ];
        save(&dir, &changed).expect("save");

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
        save(&dir, &sound()).expect("save");
        confirm(&dir).expect("confirm");

        let mut faster = sound();
        faster.publish_period_secs = 5;
        save(&dir, &faster).expect("save");

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
            meter_id: "cellar".into(),
            device_id: "dev-b".into(),
            serial: "9202686".into(),
            enabled: false,
        });
        save(&dir, &two).expect("save");
        confirm(&dir).expect("confirm");

        two.meters.reverse();
        save(&dir, &two).expect("save");

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
