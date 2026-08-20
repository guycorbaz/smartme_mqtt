//! Which of the four startup states the configuration on disk puts the process
//! in (Story 6.2 Task 7).
//!
//! # Why this is a module and not a `match` in `main.rs`
//!
//! It used to be the latter, and that was defensible while it ran **once**. It
//! runs on a loop now — Story 6.2 AC7 requires that confirming a mapping in the
//! browser starts the publishing bridge with no human intervention, so the
//! process re-decides its phase every time the configuration changes. A rule
//! that runs repeatedly and can only be exercised by spawning the binary is a
//! rule nothing checks cheaply, and this one governs whether anything reaches
//! the wire.
//!
//! # The decision is taken from the FILE, every time
//!
//! [`decide`] re-reads and re-validates. It is never handed the values a form
//! just posted, and that is the point: the file is the configuration
//! ([ADR 0023]), so a loop that trusted its own memory of what was saved would
//! be a second source of it — and the two would drift at the first write that
//! did not come through the screen.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md

use std::path::Path;

use crate::app::config::{self, ConfigErrors};
use crate::app::store::{self, Credential};
use crate::app::supervisor::BridgeConfig;

/// What the configuration on disk says the process should do next.
#[derive(Debug)]
pub enum Decision {
    /// No `config.toml` at all. A first run — not a fault, and not reported as
    /// one: the operator has done nothing wrong, they have simply not done it
    /// yet.
    Unconfigured,
    /// Valid, and no human has said the mapping is right (Story 5.3). **No MQTT
    /// session at all**, because an NBIRTH alone creates a folder in a Sparkplug
    /// host's tag tree that outlives the process.
    Unconfirmed,
    /// Valid and confirmed. Publish.
    ///
    /// Boxed because `BridgeConfig` dwarfs the other variants, and a large
    /// enum returned from a function that mostly returns the small variants
    /// costs that size at every call site.
    Publish(Box<BridgeConfig>),
    /// Present and unreadable, or present and refused by [`config::validate`].
    ///
    /// **Absence is not invalidity** ([ADR 0023] §5). Merging the two either
    /// bricks the first run — a refusal serves no screen, so the configuration
    /// could never be created — or treats a corrupt file as a fresh install and
    /// overwrites it.
    Invalid(ConfigErrors),
}

/// Read the configuration and say which phase it puts the process in.
///
/// The credential is passed in rather than read here: this decides from a file,
/// and the environment is `main.rs`'s business. The two sources have no field in
/// common, so joining them is never a merge (ADR 0023 §4).
pub fn decide(state_dir: &Path, credential: Credential) -> Decision {
    if !store::exists(state_dir) {
        return Decision::Unconfigured;
    }
    let stored = match store::read(state_dir) {
        Ok(stored) => stored,
        Err(errors) => return Decision::Invalid(errors),
    };
    let confirmed = stored.mapping_confirmed;
    let raw = store::into_raw(stored, credential, state_dir);

    // Read, then validate — and nothing in between. Every rule lives in
    // `app::config`, because the configuration has two consumers (this and the
    // web UI), and two places applying the same rules is how they drift apart.
    match config::validate(raw) {
        Err(errors) => Decision::Invalid(errors),
        // Valid but unconfirmed. Checked BEFORE the publish arm, because the
        // whole guard is that a well-formed mapping can still be pointed at the
        // wrong meter.
        Ok(_) if !confirmed => Decision::Unconfirmed,
        Ok(config) => Decision::Publish(Box::new(config)),
    }
}

/// FALSIFIED 2026-08-05. Five mutations, one per property, each asserting its
/// own text changed before anything ran — a mutation that does not apply proves
/// exactly as much as no mutation at all (2026-08-04, `rustfmt` had reflowed a
/// target and the test stayed green). Output copied, not written:
///
/// ```text
/// === A absence merged into invalidity
/// panicked at crates/smartme-bridge/src/app/phase.rs:130:9:
/// absence is not invalidity: a refusal serves no screen, so a bridge that refused here could never be configured
///
/// === B confirmation flag never read
/// panicked at crates/smartme-bridge/src/app/phase.rs:163:22:
/// confirming must lead to publishing, got Unconfirmed
///
/// === C unconfirmed guard dropped
/// panicked at crates/smartme-bridge/src/app/phase.rs:141:9:
/// a mapping nobody has looked at must open no session at all — an NBIRTH alone creates a folder a Sparkplug host keeps
///
/// === D invalid reads as first run
/// panicked at crates/smartme-bridge/src/app/phase.rs:180:9:
/// no credential is a configuration fault to report, not an empty deployment to offer a first-run screen to
///
/// === E unreadable reads as absent
/// panicked at crates/smartme-bridge/src/app/phase.rs:194:9:
/// assertion failed: matches!(decide(&dir, credential()), Decision::Invalid(_))
/// ```
///
/// **B is the one worth reading twice.** The obvious mutation for
/// `confirming_is_what_moves_the_phase_to_publishing` — dropping the
/// unconfirmed guard — makes it fail at its *pre-condition*, which would have
/// told us nothing about the property it is named for. Reading the confirmation
/// flag as a constant `false` instead makes it fail at line 163, the `panic!` in
/// the `Publish` arm, which is the claim.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::store::{StoredConfig, StoredMeter};

    fn dir(name: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("smartme_phase_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        path
    }

    fn credential() -> Credential {
        Credential {
            client_id: Some("id".to_string()),
            client_secret: Some("secret".to_string()),
        }
    }

    fn sound() -> StoredConfig {
        StoredConfig {
            created_ms: None,
            last_change_ms: None,
            schema_version: store::SCHEMA_VERSION,
            group_id: "Site".to_string(),
            node_id: "Bridge".to_string(),
            broker_host: "broker".to_string(),
            broker_port: 1883,
            publish_period_secs: 30,
            api_base: None,
            log_dir: None,
            log_keep: None,
            mapping_confirmed: false,
            ui_port: None,
            meters: vec![StoredMeter {
                priority: false,
                meter_id: "garage".to_string(),
                device_id: "abc".to_string(),
                serial: "9202685".to_string(),
                enabled: true,
            }],
        }
    }

    #[test]
    fn no_file_is_a_first_run_and_not_a_fault() {
        let dir = dir("absent");
        assert!(
            matches!(decide(&dir, credential()), Decision::Unconfigured),
            "absence is not invalidity: a refusal serves no screen, so a bridge \
             that refused here could never be configured"
        );
    }

    #[test]
    fn a_valid_unconfirmed_mapping_does_not_publish() {
        let dir = dir("unconfirmed");
        store::save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("write");
        assert!(
            matches!(decide(&dir, credential()), Decision::Unconfirmed),
            "a mapping nobody has looked at must open no session at all — an \
             NBIRTH alone creates a folder a Sparkplug host keeps"
        );
    }

    #[test]
    fn confirming_is_what_moves_the_phase_to_publishing() {
        let dir = dir("confirmed");
        store::save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("write");
        assert!(matches!(decide(&dir, credential()), Decision::Unconfirmed));

        store::confirm(&dir).expect("confirm");

        // The SAME directory, re-read: this is the transition Story 6.2 AC7
        // exists to make happen without a human touching a terminal.
        match decide(&dir, credential()) {
            Decision::Publish(config) => {
                assert_eq!(config.group_id, "Site");
                assert_eq!(config.meters.len(), 1);
            }
            other => panic!("confirming must lead to publishing, got {other:?}"),
        }
    }

    /// The credential is joined here and nowhere else, so a missing one has to
    /// present as an invalid configuration rather than as a first run — the two
    /// call for opposite actions.
    #[test]
    fn a_confirmed_mapping_with_no_credential_is_invalid_not_unconfigured() {
        let dir = dir("no_credential");
        store::save(&dir, &sound(), crate::domain::UtcMillis(1_784_984_793_000)).expect("write");
        store::confirm(&dir).expect("confirm");

        let empty = Credential {
            client_id: None,
            client_secret: None,
        };
        assert!(
            matches!(decide(&dir, empty), Decision::Invalid(_)),
            "no credential is a configuration fault to report, not an empty \
             deployment to offer a first-run screen to"
        );
    }

    /// The seam `store::exists` guards: a file that is there and cannot be read
    /// must never be mistaken for one that is not there, or the next save would
    /// overwrite it.
    #[test]
    fn an_unreadable_file_is_refused_rather_than_treated_as_absent() {
        let dir = dir("garbage");
        std::fs::write(store::config_path(&dir), b"this is not toml {{{").expect("write");
        assert!(matches!(decide(&dir, credential()), Decision::Invalid(_)));
    }
}
