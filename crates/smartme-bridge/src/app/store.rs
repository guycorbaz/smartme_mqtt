//! Where the configuration rests, and what it costs to read it back wrong
//! (Story 5.2, [ADR 0022]).
//!
//! # Two files, split by sensitivity
//!
//! `config.toml` (`0644`) holds everything an operator might want to read while
//! diagnosing something — meters, period, broker. `secrets.toml` (`0600`) holds
//! the smart-me credentials and nothing else.
//!
//! One file would be simpler and could not desynchronise. It was rejected
//! because it makes the *whole* configuration unreadable without privileges,
//! including for checking which topic a meter maps to. The habit that forms is
//! `sudo cat`, and the habit that follows is a credential on a screen during a
//! support conversation. The cost of the split — the two files can disagree — is
//! handled here rather than hoped away: a meter in one and not the other is a
//! validation fault like any other.
//!
//! # The mode is verified, not assumed
//!
//! [`load`] refuses a `secrets.toml` that is readable by group or other. The
//! bridge sets `0600` when it writes, so this looks redundant — it is not. On
//! this very deployment the mode bits once read `drwxrwxrwx` while a Synology
//! ACL denied the process access ([#41]): **the displayed mode was not the
//! enforced permission.** A mode set at creation says nothing about what a
//! restore, a volume remount, an `umask` or a `docker cp` did afterwards.
//!
//! # An older file must not be read by guesswork
//!
//! Every stored struct is `deny_unknown_fields` and carries a
//! [`SCHEMA_VERSION`]. Serde's defaults are the trap: unknown fields are ignored
//! and missing ones take `Default`, so a renamed field would read as *absent*,
//! take its default, and the bridge would start on a configuration nobody wrote
//! — publishing at 30 s because the period silently reverted. Refusing is the
//! only honest answer until a migration exists to be the other one.
//!
//! [ADR 0022]: ../../../docs/adr/0022-secrets-rest-in-a-separate-0600-file.md
//! [#41]: https://github.com/guycorbaz/smartme_mqtt/issues/41

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::config::{ConfigErrors, Fault, RawConfig, RawMeter};
use crate::persist;

/// The shape of what is on disk. Bumped whenever a field is added, renamed or
/// removed — see the module docs for why "read it and hope" is not available.
pub const SCHEMA_VERSION: u32 = 1;

/// Mode for the file that holds credentials.
pub const SECRETS_MODE: u32 = 0o600;

/// Bits that must NOT be set on the secrets file: any read, write or execute
/// permission for group or other.
const FORBIDDEN_BITS: u32 = 0o077;

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.toml")
}

pub fn secrets_path(dir: &Path) -> PathBuf {
    dir.join("secrets.toml")
}

/// Non-sensitive settings, as stored.
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
    pub meters: Vec<StoredMeter>,
}

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

/// The credentials, alone in their own file.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoredSecrets {
    pub schema_version: u32,
    pub client_id: String,
    pub client_secret: String,
}

/// Hand-written so that `{:?}` cannot leak the secret.
///
/// A `#[derive(Debug)]` here would put the credential into every error that
/// formats a struct containing one, which is precisely the accident ADR 0019
/// exists to make impossible. Derives are the default; this is the exception
/// that has to be written down.
impl std::fmt::Debug for StoredSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredSecrets")
            .field("schema_version", &self.schema_version)
            .field("client_id", &"<redacted>")
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

fn fault(field: &str, problem: String) -> Fault {
    Fault {
        field: field.to_string(),
        env_var: None,
        problem,
    }
}

/// Is there a stored configuration at all?
pub fn exists(dir: &Path) -> bool {
    config_path(dir).exists()
}

/// Refuse a secrets file anyone but the owner can touch.
///
/// Returns the fault rather than logging it, so it joins every other problem in
/// the one report the operator reads (Story 5.1 AC2).
#[cfg(unix)]
fn check_mode(path: &Path) -> Option<Fault> {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = std::fs::metadata(path).ok()?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & FORBIDDEN_BITS != 0 {
        return Some(fault(
            "secrets file permissions",
            format!(
                "{} is mode {mode:04o}; it must be {SECRETS_MODE:04o}. This is checked rather \
                 than assumed: on this deployment the mode bits once read drwxrwxrwx while an \
                 ACL denied the process access, so a mode set at creation says nothing about \
                 what a restore, a remount, an umask or a docker cp did afterwards",
                path.display()
            ),
        ));
    }
    None
}

#[cfg(not(unix))]
fn check_mode(_path: &Path) -> Option<Fault> {
    None
}

/// Read both files into the same [`RawConfig`] the environment produces, so a
/// single [`crate::app::config::validate`] governs both sources.
///
/// **The merge does not happen here, and that is deliberate.** Whatever comes
/// back goes through the same validation as the bootstrap path, which is the one
/// guarantee that a value accepted in a browser is a value the bridge will boot
/// on.
pub fn load(dir: &Path) -> Result<RawConfig, ConfigErrors> {
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

    if let Some(problem) = check_mode(&secrets_path(dir)) {
        faults.push(problem);
    }

    let secrets: Option<StoredSecrets> = match persist::load(&secrets_path(dir)) {
        Ok(secrets) => Some(secrets),
        Err(error) => {
            // The error may embed a TOML fragment, so the file is named and the
            // cause is given by kind alone — never by echoing the content of a
            // file whose whole purpose is to hold a credential.
            faults.push(fault(
                "stored secrets",
                format!(
                    "{} could not be read ({}). The configuration and the secrets are two \
                     files and can disagree; that is the accepted cost of keeping \
                     config.toml readable for diagnosis",
                    secrets_path(dir).display(),
                    error.kind()
                ),
            ));
            None
        }
    };

    for (name, version) in [
        (
            "stored configuration",
            config.as_ref().map(|c| c.schema_version),
        ),
        ("stored secrets", secrets.as_ref().map(|s| s.schema_version)),
    ] {
        if let Some(version) = version
            && version != SCHEMA_VERSION
        {
            faults.push(fault(
                name,
                format!(
                    "was written by schema version {version}, this build reads \
                     {SCHEMA_VERSION}. There is no migration, so it is refused rather than \
                     read by guesswork — an unrecognised field would otherwise take its \
                     default and the bridge would run on settings nobody wrote"
                ),
            ));
        }
    }

    if !faults.is_empty() {
        return Err(ConfigErrors(faults));
    }

    let config = config.expect("no faults means the configuration was read");
    let secrets = secrets.expect("no faults means the secrets were read");

    Ok(RawConfig {
        api_base: config.api_base,
        client_id: Some(secrets.client_id),
        client_secret: Some(secrets.client_secret),
        group_id: Some(config.group_id),
        node_id: Some(config.node_id),
        broker_host: Some(config.broker_host),
        broker_port: Some(config.broker_port.to_string()),
        state_dir: Some(dir.display().to_string()),
        publish_period_secs: Some(config.publish_period_secs.to_string()),
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
    })
}

/// Write both files, each with its own mode, atomically.
///
/// The secrets file is created `0600` **by the open call**, not chmod'ed after —
/// see [`crate::persist::persist_atomic_with_mode`].
pub fn save(dir: &Path, config: &StoredConfig, secrets: &StoredSecrets) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    persist::persist_atomic_with_mode(&secrets_path(dir), secrets, SECRETS_MODE)?;
    persist::persist_atomic(&config_path(dir), config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("smartme_store_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        path
    }

    fn sound() -> (StoredConfig, StoredSecrets) {
        (
            StoredConfig {
                schema_version: SCHEMA_VERSION,
                group_id: "Group".into(),
                node_id: "Node".into(),
                broker_host: "broker".into(),
                broker_port: 1883,
                publish_period_secs: 30,
                api_base: None,
                meters: vec![StoredMeter {
                    meter_id: "meter-a".into(),
                    device_id: "dev-a".into(),
                    serial: "9202685".into(),
                    enabled: true,
                }],
            },
            StoredSecrets {
                schema_version: SCHEMA_VERSION,
                client_id: "id".into(),
                client_secret: "s3cr3t-do-not-print".into(),
            },
        )
    }

    #[test]
    fn a_saved_configuration_round_trips_through_the_same_validation_as_the_environment() {
        let dir = dir("roundtrip");
        let (config, secrets) = sound();
        save(&dir, &config, &secrets).expect("save");
        let raw = load(&dir).expect("load");
        let validated = crate::app::config::validate(raw).expect("validates");
        assert_eq!(validated.meters.len(), 1);
        assert_eq!(validated.poll.interval.as_secs(), 30);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC2. The mode is set by the open call, so this asserts the property that
    /// makes the ADR's *verified, not assumed* claim meaningful in the first
    /// place — that what we write is already right.
    #[cfg(unix)]
    #[test]
    fn the_secrets_file_is_created_0600_not_tightened_afterwards() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = dir("mode");
        let (config, secrets) = sound();
        save(&dir, &config, &secrets).expect("save");
        let mode = std::fs::metadata(secrets_path(&dir))
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, SECRETS_MODE, "got {mode:04o}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The check that exists because a mode set at creation proves nothing about
    /// what happened to the file afterwards.
    #[cfg(unix)]
    #[test]
    fn a_group_readable_secrets_file_is_refused() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = dir("loose");
        let (config, secrets) = sound();
        save(&dir, &config, &secrets).expect("save");
        std::fs::set_permissions(secrets_path(&dir), std::fs::Permissions::from_mode(0o644))
            .expect("loosen");
        let errors = load(&dir).expect_err("a readable secrets file must be refused");
        assert!(
            errors
                .0
                .iter()
                .any(|f| f.field == "secrets file permissions"),
            "got {:?}",
            errors.0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC5 — the trap is that serde would have made this SUCCEED.
    #[test]
    fn a_file_from_another_schema_version_is_refused_rather_than_defaulted() {
        let dir = dir("schema");
        let (mut config, secrets) = sound();
        config.schema_version = SCHEMA_VERSION + 1;
        save(&dir, &config, &secrets).expect("save");
        let errors = load(&dir).expect_err("a future schema must be refused");
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
        let (config, secrets) = sound();
        save(&dir, &config, &secrets).expect("save");

        // At the ROOT — before any table header, or TOML makes it a member of the
        // last table and this asserts something else entirely.
        let text = std::fs::read_to_string(config_path(&dir)).expect("read");
        std::fs::write(
            config_path(&dir),
            format!("publish_period_seconds = 300\n{text}"),
        )
        .expect("write");
        let errors = load(&dir).expect_err("an unknown ROOT field must be refused");
        assert!(
            errors.0.iter().any(|f| f.field == "stored configuration"),
            "got {:?}",
            errors.0
        );

        // And inside a meter, which is a different struct and a different derive.
        save(&dir, &config, &secrets).expect("save");
        let mut text = std::fs::read_to_string(config_path(&dir)).expect("read");
        text.push_str("\nserial_number = \"9202685\"\n");
        std::fs::write(config_path(&dir), text).expect("write");
        let errors = load(&dir).expect_err("an unknown METER field must be refused");
        assert!(
            errors.0.iter().any(|f| f.field == "stored configuration"),
            "got {:?}",
            errors.0
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC3 — two files can disagree, and the story accepted that cost on
    /// condition it be handled.
    #[test]
    fn a_missing_secrets_file_is_a_fault_and_not_a_panic() {
        let dir = dir("desync");
        let (config, secrets) = sound();
        save(&dir, &config, &secrets).expect("save");
        std::fs::remove_file(secrets_path(&dir)).expect("remove");
        let errors = load(&dir).expect_err("a missing secrets file must be refused");
        assert!(
            errors.0.iter().any(|f| f.field == "stored secrets"),
            "got {:?}",
            errors.0
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC6, and the reason `Debug` is hand-written on `StoredSecrets`.
    #[test]
    fn the_secret_reaches_neither_debug_nor_a_fault() {
        let (_, secrets) = sound();
        let debugged = format!("{secrets:?}");
        assert!(
            !debugged.contains("s3cr3t-do-not-print"),
            "a derived Debug would have leaked it: {debugged}"
        );
        assert!(
            debugged.contains("<redacted>"),
            "the absence assertion above needs the struct to have rendered at all: {debugged}"
        );

        let dir = dir("leak");
        let (config, secrets) = sound();
        save(&dir, &config, &secrets).expect("save");
        std::fs::write(secrets_path(&dir), "schema_version = 1\nnot-toml").expect("corrupt");
        let errors = load(&dir).expect_err("corrupt secrets must be refused");
        assert!(
            !format!("{errors}").contains("s3cr3t-do-not-print"),
            "the parse error echoed the file: {errors}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
