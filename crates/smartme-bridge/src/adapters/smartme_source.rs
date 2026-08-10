//! `SmartMeCloudSource` — the real [`Source`] over the smart-me cloud (Story 1.7).
//!
//! THE unit-conversion boundary: power → [`Kw`], energy → [`Kwh`] happen here and
//! nowhere else. Fail-closed (FR8 thin slice): an unknown/mismatched unit or a
//! non-finite value marks the `Measurement` `Quality::Bad` — no guessed value is
//! ever produced, and the carrier value for a Bad reading is a documented
//! non-value (0.0), never something plausible.
//!
//! Token lifecycle (ADR 0009): client-credentials tokens are anchored against the
//! injected [`Clock`] at exchange time (minus a safety margin); a 401 arriving on
//! a previously-valid token triggers ONE refresh + retry — a second rejection is
//! the real thing and surfaces as `Fatal`.

use std::future::Future;
use std::sync::Arc;

use smart_me_client::{Device, SmartMeClient, SmartMeError, TokenState};

use crate::core::clock::{Clock, MonotonicMs};
use crate::core::source::{Reading, Source, SourceError};
use crate::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};

/// Safety margin subtracted from the reported token lifetime: refresh a little
/// early rather than race the expiry in flight.
const TOKEN_MARGIN_MS: i64 = 30_000;

/// Floor on the usable lifetime of a token. A server reporting a lifetime at or
/// below [`TOKEN_MARGIN_MS`] would otherwise expire the token the instant it is
/// minted, turning every poll into a fresh OAuth exchange (a self-inflicted
/// hammering of the token endpoint). We keep such a token for this long instead
/// and let the 401 refresh-retry path handle the fallout.
const TOKEN_MIN_LIFETIME_MS: i64 = 5_000;

/// The value carried by a `Quality::Bad` measurement — a documented non-value.
/// Consumers must not use it (that is what `Bad` means); it is 0.0 so that a
/// pipeline bug that ignores quality produces an obviously-flat trace, not a
/// plausible one.
const BAD_CARRIER: f64 = 0.0;

struct AnchoredToken {
    token: TokenState,
    /// MONOTONIC instant after which the token is treated as expired. A token
    /// lifetime is a duration, so it is anchored on the monotonic clock: an NTP
    /// step must not stretch a dead token's apparent validity (nor discard a
    /// live one).
    expires_at: MonotonicMs,
}

/// The production meter feed: one configured smart-me device polled over TLS.
pub struct SmartMeCloudSource {
    client: SmartMeClient,
    clock: Arc<dyn Clock + Send + Sync>,
    /// The logical meter this source serves.
    meter: MeterId,
    /// The serial the CONFIGURATION declares for that meter — the one
    /// `supervisor` births the Sparkplug device under. See
    /// [`UnverifiedReading::verify`] for why it has to be carried down here.
    declared_serial: Serial,
    /// The smart-me device UUID backing that meter.
    device_id: String,
    token: Option<AnchoredToken>,
}

impl SmartMeCloudSource {
    /// Wires the source to one meter/device pair.
    pub fn new(
        client: SmartMeClient,
        clock: Arc<dyn Clock + Send + Sync>,
        meter: MeterId,
        declared_serial: Serial,
        device_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            clock,
            meter,
            declared_serial,
            device_id: device_id.into(),
            token: None,
        }
    }

    /// Ensures a live bearer token in client-credentials mode (no-op for Basic).
    /// Anchors expiry on the monotonic clock the moment the exchange returns.
    /// Returns `true` when it minted a NEW token (so the caller knows a
    /// subsequent 401 cannot be an expiry race — see [`Source::fetch`]).
    async fn ensure_token(&mut self) -> Result<bool, SourceError> {
        if !self.client.uses_client_credentials() {
            return Ok(false);
        }
        let now = self.clock.monotonic();
        if let Some(t) = &self.token {
            if now < t.expires_at {
                return Ok(false);
            }
        }
        let token = self.client.fetch_token().await.map_err(map_error)?;
        let usable_ms = token
            .expires_in_s
            .saturating_mul(1_000)
            .saturating_sub(TOKEN_MARGIN_MS)
            .max(TOKEN_MIN_LIFETIME_MS);
        self.token = Some(AnchoredToken {
            expires_at: MonotonicMs(now.0.saturating_add(usable_ms)),
            token,
        });
        Ok(true)
    }

    async fn fetch_once(&mut self) -> Result<UnverifiedReading, SmartMeError> {
        let token = self.token.as_ref().map(|t| &t.token);
        let capture = self.client.get_device(&self.device_id, token).await?;
        Ok(map_device(
            &capture.device,
            capture.http_date_ms,
            &self.meter,
        ))
    }
}

/// A reading whose device identity has not been checked yet.
///
/// # Why a type and not a call
///
/// [`SmartMeCloudSource::fetch`] has two success paths — the ordinary one and
/// the refresh-and-retry after a 401 — and a check written on one of them would
/// hold for every poll until a token expired and silently stop holding
/// afterwards. That is the shape `node_metrics` in the publisher was
/// restructured to remove: *building the list once removes the omission rather
/// than testing for it*. `fetch_once` is the only producer of a [`Reading`] in
/// this module and it cannot produce one at all — only this — so dropping the
/// check does not compile.
///
/// **The guarantee is exactly that, and no more.** It was written here as *"the
/// compiler, not a reviewer, puts `verify` on both paths"* and that was too
/// strong: the field is private to this module, not to this type, so
/// `unverified.0` compiles and was measured to. What is closed is the
/// ACCIDENTAL omission — the branch somebody forgets — which is the one that has
/// happened in this repository. A deliberate unwrap stays possible and is one
/// word long in a diff. Saying so here rather than inheriting a claim the
/// measurement does not support.
#[must_use]
struct UnverifiedReading(Reading);

impl UnverifiedReading {
    /// Binds the reading to the device the configuration declared, or refuses it.
    ///
    /// # The failure this exists to make loud
    ///
    /// The DBIRTH declares `meters[].serial` from the file
    /// (`supervisor::run_with_control`); every DDATA is routed by the serial the
    /// smart-me RESPONSE carries (`SparkplugPublisher::publish`). Nothing
    /// compared the two until 2026-08-09, so a serial that was merely *legal*
    /// but wrong produced a bridge that looked perfect and published nothing:
    /// the fetch succeeded, the oracle said `Fresh`, the heartbeat ticked, `/`
    /// named no fault and `/healthz` answered `200` with an empty
    /// `failed_sources` — while every reading was discarded as
    /// `DroppedUndeclaredDevice` behind one `warn` per period.
    ///
    /// `config::check_serial` already refuses the one shape of this that had
    /// actually happened here (a leading zero) and says in as many words that
    /// *"the real requirement is «the serial must be the one smart-me reports»,
    /// which cannot be checked offline"*. It cannot — but it can be checked
    /// ONLINE, on the very response the bridge already holds, and that is all
    /// this is. The offline proxy stays: it refuses before a single API call,
    /// this refuses on the first answer.
    ///
    /// # Fatal, deliberately
    ///
    /// A serial does not drift back on its own, so `Transient` would poll a
    /// misconfigured meter for ever while publishing `Stale` — a fault reported
    /// as weather. `Fatal` latches [`State::Failed`](crate::core::state_machine::State),
    /// which names the meter on `/` and in `failed_sources`, and only a restart
    /// clears it. That is not a limitation here: `reconfigure::classify_meters`
    /// already costs a serial change a `ProcessRestart`, so the repair and the
    /// latch ask for the same thing.
    ///
    /// # Checked on every reading, not only the first
    ///
    /// The comparison costs nothing and the first answer is not the only one
    /// that can carry the wrong device — a `device_id` re-pointed on the
    /// smart-me side would otherwise be adopted silently.
    fn verify(
        self,
        declared: &Serial,
        meter: &MeterId,
        device_id: &str,
    ) -> Result<Reading, SourceError> {
        let reported = &self.0.value.serial;
        if reported != declared {
            return Err(SourceError::Fatal {
                reason: format!(
                    "meter {meter} is declared with serial {declared}, but smart-me device \
                     {device_id} reports serial {reported}. The device is born on the wire \
                     under the declared serial and every reading is routed by the reported \
                     one, so nothing this meter reads would ever reach the broker. Correct \
                     the serial or the device id in the configuration, then restart"
                ),
            });
        }
        Ok(self.0)
    }
}

impl Source for SmartMeCloudSource {
    fn fetch(
        &mut self,
        meter: &MeterId,
    ) -> impl Future<Output = Result<Reading, SourceError>> + Send {
        let requested = meter.clone();
        async move {
            if requested != self.meter {
                // A wiring bug, not a network condition: fail fatal and loud.
                return Err(SourceError::Fatal {
                    reason: format!(
                        "source wired for meter {} was asked for {}",
                        self.meter, requested
                    ),
                });
            }
            // `minted_now` is the discriminator: a token created in THIS call
            // cannot have expired in flight, so a 401 on it is the real thing —
            // retrying would only burn a second exchange and delay the Fatal.
            let minted_now = self.ensure_token().await?;
            let reusing_token = self.token.is_some() && !minted_now;
            let unverified = match self.fetch_once().await {
                Ok(reading) => reading,
                Err(SmartMeError::AuthRejected { .. }) if reusing_token => {
                    // ADR 0009: exactly one refresh + retry after a
                    // PREVIOUSLY-valid token — expiry can race the request in
                    // flight. A second rejection surfaces as the fatal it is.
                    self.token = None;
                    self.ensure_token().await?;
                    self.fetch_once().await.map_err(map_error)?
                }
                Err(e) => return Err(map_error(e)),
            };
            // ONE call, covering both paths above — see `UnverifiedReading`.
            unverified.verify(&self.declared_serial, &self.meter, &self.device_id)
        }
    }
}

/// The client's own classification is authoritative: fatal → `Fatal`,
/// timeout → `Timeout`, everything else transient.
fn map_error(e: SmartMeError) -> SourceError {
    if e.is_fatal() {
        SourceError::Fatal {
            reason: e.to_string(),
        }
    } else if matches!(e, SmartMeError::Timeout) {
        SourceError::Timeout
    } else {
        SourceError::Transient {
            reason: e.to_string(),
        }
    }
}

/// Pure mapping: smart-me device → [`Reading`]. Units converted HERE only;
/// unknown unit or non-finite value → `Quality::Bad` with the documented
/// non-value carrier. An unparseable `ValueDate` is a `Bad` reading pinned to
/// the epoch floor (no timestamp can be invented for it) — the state machine's
/// plausibility guard keeps it un-fresh forever.
fn map_device(device: &Device, http_date_ms: Option<i64>, meter: &MeterId) -> UnverifiedReading {
    let power = convert_power(device.active_power, &device.active_power_unit);
    let energy = convert_energy(device.counter_reading, &device.counter_reading_unit);
    let value_date = smart_me_client::parse_value_date(&device.value_date);

    let (power, energy, value_date, quality) = match (power, energy, value_date) {
        (Some(p), Some(e), Some(vd)) => (p, e, UtcMillis(vd), Quality::Good),
        (p, e, vd) => (
            p.unwrap_or(Kw(BAD_CARRIER)),
            e.unwrap_or(Kwh(BAD_CARRIER)),
            UtcMillis(vd.unwrap_or(0)),
            Quality::Bad,
        ),
    };
    UnverifiedReading(Reading {
        value: Measurement {
            meter: meter.clone(),
            // The serial the RESPONSE carries, never the configured one. That
            // distinction is the whole reason `UnverifiedReading` exists: copying
            // the declared serial in here would make the two agree by
            // construction and the bridge would publish a mis-mapped meter's
            // readings under the name of the meter it was asked for.
            serial: Serial::new(device.serial.to_string()),
            power,
            energy,
            value_date,
            quality,
        },
        http_date: http_date_ms.map(UtcMillis),
    })
}

/// Rescales `value` from `unit` to the canonical unit named by `base`. Units are
/// matched EXACTLY on purpose: a casing drift (`"KW"`) is a contract change that
/// must surface as `Bad`, never be silently absorbed. Finiteness is checked both
/// before and after the arithmetic — a finite input can still overflow the ×1000
/// of the mega unit.
fn rescale(value: f64, unit: &str, base: &str, milli: &str, mega: &str) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let out = if unit == base {
        value
    } else if unit == milli {
        value / 1_000.0
    } else if unit == mega {
        value * 1_000.0
    } else {
        return None;
    };
    out.is_finite().then_some(out)
}

/// Power → kW. Exact-match units, fail-closed; non-finite values refused.
fn convert_power(value: f64, unit: &str) -> Option<Kw> {
    rescale(value, unit, "kW", "W", "MW").map(Kw)
}

/// Energy → kWh. Exact-match units, fail-closed; non-finite values refused.
fn convert_energy(value: f64, unit: &str) -> Option<Kwh> {
    rescale(value, unit, "kWh", "Wh", "MWh").map(Kwh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(power_unit: &str, energy_unit: &str, value_date: &str) -> Device {
        // The captured METER-A shape (fixtures are the contract-of-record).
        serde_json::from_str(&format!(
            r#"{{"Id":"a1a1a1a1-b2b2-c3c3-d4d4-000000000001","Name":"METER-A",
                "Serial":30000001,"ActivePower":0.018,"ActivePowerUnit":"{power_unit}",
                "CounterReading":4843.822,"CounterReadingUnit":"{energy_unit}",
                "ValueDate":"{value_date}"}}"#
        ))
        .expect("test device json")
    }

    const VD: &str = "2026-07-25T13:06:32.0500519Z";
    /// The serial the fixture device reports. Every identity test below states
    /// what it declares AGAINST this, rather than sharing one constant for both
    /// sides — a shared constant would make the comparison agree with itself.
    const REPORTED: &str = "30000001";

    /// The mapping alone, for the tests that are about units and timestamps.
    ///
    /// The identity check has its own tests below and is deliberately not folded
    /// in here: a helper that verified would make every unit test depend on a
    /// serial none of them is about.
    fn mapped(device: &Device, http_date_ms: Option<i64>, meter: &MeterId) -> Reading {
        map_device(device, http_date_ms, meter).0
    }

    #[test]
    fn known_units_convert_and_stay_good() {
        let m = MeterId::new("m1");
        let r = mapped(&device("kW", "kWh", VD), Some(1_000), &m);
        assert_eq!(r.value.quality, Quality::Good);
        assert_eq!(r.value.power, Kw(0.018));
        assert_eq!(r.value.energy, Kwh(4_843.822));
        assert_eq!(r.http_date, Some(UtcMillis(1_000)));
        assert_eq!(r.value.serial.as_str(), "30000001");

        let w = mapped(&device("W", "Wh", VD), None, &m);
        // Same arithmetic as the converter — literal decimals differ in the
        // last binary digit.
        assert_eq!(w.value.power, Kw(0.018 / 1_000.0));
        assert_eq!(w.value.energy, Kwh(4_843.822 / 1_000.0));

        let mw = mapped(&device("MW", "MWh", VD), None, &m);
        assert_eq!(mw.value.power, Kw(18.0));
        assert_eq!(mw.value.energy, Kwh(4_843_822.0));
    }

    #[test]
    fn unknown_unit_fails_closed_to_bad_with_non_value_carrier() {
        let m = MeterId::new("m1");
        for (pu, eu) in [("kVA", "kWh"), ("kW", "MJ"), ("", ""), ("KW", "kWh")] {
            let r = mapped(&device(pu, eu, VD), Some(1_000), &m);
            assert_eq!(r.value.quality, Quality::Bad, "units {pu:?}/{eu:?}");
            // No guessed value: at least one side is the documented non-value.
            assert!(r.value.power == Kw(0.0) || r.value.energy == Kwh(0.0));
        }
    }

    #[test]
    fn case_sensitive_units_are_deliberate() {
        // "KW" is not "kW": a casing drift in the API is a contract change and
        // must surface as Bad, not be silently absorbed.
        let m = MeterId::new("m1");
        let r = mapped(&device("KW", "kWh", VD), None, &m);
        assert_eq!(r.value.quality, Quality::Bad);
    }

    #[test]
    fn non_finite_values_fail_closed() {
        assert_eq!(convert_power(f64::NAN, "kW"), None);
        assert_eq!(convert_power(f64::INFINITY, "W"), None);
        assert_eq!(convert_energy(f64::NEG_INFINITY, "kWh"), None);
    }

    #[test]
    fn scaling_overflow_fails_closed_too() {
        // A finite input can still overflow the ×1000 of the mega units: the
        // result must be refused, not published as an infinite "Good" value.
        assert_eq!(convert_power(f64::MAX, "MW"), None);
        assert_eq!(convert_energy(f64::MAX, "MWh"), None);
        // ...while the same magnitude in the base unit is fine.
        assert_eq!(convert_power(f64::MAX, "kW"), Some(Kw(f64::MAX)));
    }

    #[test]
    fn unparseable_value_date_is_bad_pinned_to_the_floor() {
        let m = MeterId::new("m1");
        let r = mapped(&device("kW", "kWh", "not-a-date"), Some(1_000), &m);
        assert_eq!(r.value.quality, Quality::Bad);
        assert_eq!(r.value.value_date, UtcMillis(0));
    }

    #[test]
    fn a_bad_reading_stays_bad_through_the_oracle() {
        // The adapter's Bad must not be relabeled Stale by the timestamp guards
        // (the epoch-pinned value_date would otherwise read as a huge age).
        use crate::core::state_machine::{Policy, State};
        let m = MeterId::new("m1");
        let r = mapped(&device("kVA", "kWh", "not-a-date"), Some(1_000), &m);
        let policy = Policy::DEFAULT;
        let (state, published) =
            policy.step(State::initial(), &Ok(r), UtcMillis(1_784_984_793_000));
        assert_eq!(state, State::Stale);
        assert_eq!(published.quality(), Quality::Bad);
        // Story 2.1: the cause now travels with it, and it is the source's own
        // fail-closed conversion rather than anything about the timestamps.
        assert_eq!(
            published.cause(),
            Some(crate::core::oracle::Cause::ValueUnusable)
        );
    }

    /// **The reading is bound to the device that was declared, or refused.**
    ///
    /// The failure it closes was silent in the worst way available here: the
    /// fetch succeeds, so the oracle says `Fresh`, the heartbeat ticks, `/` names
    /// no fault and `/healthz` answers `200` with an empty `failed_sources` —
    /// while the publisher discards every reading as `DroppedUndeclaredDevice`,
    /// because the DBIRTH went out under the CONFIGURED serial and the DDATA is
    /// routed by the REPORTED one. A bridge that looks perfect and publishes
    /// nothing.
    ///
    /// FALSIFIED 2026-08-09, three mutations. Copied from the runs.
    ///
    /// **(1) The guard catches nothing** (`if reported != declared` → `if
    /// false`). Two tests red, and the panic prints the harm rather than a
    /// number:
    ///
    /// ```text
    /// thread '…a_device_that_is_not_the_declared_one_is_refused_fatally' (357) panicked at
    /// crates/smartme-bridge/src/adapters/smartme_source.rs:445:14:
    /// a reading from another device must not be accepted: Reading { value: Measurement {
    /// meter: MeterId("cellar"), serial: Serial("30000001"), … quality: Good } … }
    ///
    /// test result: FAILED. 9 passed; 2 failed
    /// ```
    ///
    /// `meter: MeterId("cellar")` carrying `Serial("30000001")` at quality
    /// `Good` IS the defect, produced rather than argued: the cellar's reading
    /// is the garage's device, and everything downstream would have believed it.
    /// `the_device_that_was_declared_is_accepted_unchanged` stayed GREEN, so the
    /// mutation removed the refusal and nothing else.
    ///
    /// **(2) The guard refuses everything** (`!=` → `==`). THREE red, and the
    /// third is the control: a guard that took the whole fleet off the wire
    /// would have passed mutation 1's test. `test result: FAILED. 8 passed; 3
    /// failed`.
    ///
    /// **(3) The call is dropped from `fetch`** (`unverified.verify(…)` →
    /// `Ok(unverified)`). Not a red test — a red BUILD, which is the property
    /// [`UnverifiedReading`] exists for:
    ///
    /// ```text
    /// error[E0271]: expected … to be a future that resolves to `Result<Reading, SourceError>`,
    /// but it resolves to `Result<UnverifiedReading, SourceError>`
    /// ```
    ///
    /// A fourth attempt measured the LIMIT of that guarantee rather than
    /// assuming it: `Ok(unverified.0)` compiles clean. The type closes the
    /// forgotten branch, not a deliberate unwrap — recorded on the type itself.
    #[test]
    fn a_device_that_is_not_the_declared_one_is_refused_fatally() {
        let meter = MeterId::new("cellar");
        let declared = Serial::new("30000002"); // the neighbour's serial
        let error = map_device(&device("kW", "kWh", VD), Some(1_000), &meter)
            .verify(&declared, &meter, "a1a1a1a1-b2b2-c3c3-d4d4-000000000001")
            .expect_err("a reading from another device must not be accepted");

        // FATAL, not Transient: a serial does not come back on its own, and a
        // transient verdict would poll a misconfigured meter for ever while
        // publishing Stale — a configuration fault reported as weather.
        let SourceError::Fatal { reason } = error else {
            panic!(
                "must be Fatal, so the meter latches Failed and is named on the screen: {error:?}"
            );
        };
        // The message must send the operator to the two fields that can be
        // wrong. "Something is wrong" sends them to the logs; these send them to
        // the line of the form that needs editing.
        for expected in [
            declared.as_str(),
            REPORTED,
            "a1a1a1a1-b2b2-c3c3-d4d4-000000000001",
            "cellar",
        ] {
            assert!(
                reason.contains(expected),
                "the refusal must name {expected}, or it cannot be acted on: {reason}"
            );
        }
    }

    /// **The control**, and it is not a formality: a guard that refused every
    /// reading would satisfy the test above and take the whole fleet off the
    /// wire. This is the assertion that says the refusal has a subject.
    #[test]
    fn the_device_that_was_declared_is_accepted_unchanged() {
        let meter = MeterId::new("garage");
        let declared = Serial::new(REPORTED);
        let reading = map_device(&device("kW", "kWh", VD), Some(1_000), &meter)
            .verify(&declared, &meter, "a1a1a1a1-b2b2-c3c3-d4d4-000000000001")
            .expect("the declared device must pass");
        assert_eq!(reading.value.serial, declared);
        assert_eq!(
            reading.value.quality,
            Quality::Good,
            "verifying identity must not touch the reading's own quality"
        );
        assert_eq!(reading.value.power, Kw(0.018));
    }

    /// A `Bad` reading is still checked for identity, and the identity verdict
    /// wins.
    ///
    /// The two are independent judgements and the order matters: a mis-mapped
    /// meter whose units also fail would otherwise be published as `Bad` under a
    /// device that is not it — the wrong tag carrying an honest-looking fault,
    /// which is harder to diagnose than either problem alone.
    #[test]
    fn a_bad_reading_from_the_wrong_device_is_still_a_wiring_fault() {
        let meter = MeterId::new("cellar");
        let error = map_device(&device("kVA", "kWh", VD), None, &meter)
            .verify(&Serial::new("30000002"), &meter, "some-uuid")
            .expect_err("identity is decided before quality is anybody's problem");
        assert!(matches!(error, SourceError::Fatal { .. }));
    }

    #[test]
    fn error_mapping_follows_the_client_classification() {
        assert!(matches!(
            map_error(SmartMeError::AuthRejected { status: 401 }),
            SourceError::Fatal { .. }
        ));
        assert!(matches!(
            map_error(SmartMeError::Timeout),
            SourceError::Timeout
        ));
        assert!(matches!(
            map_error(SmartMeError::HttpStatus { status: 503 }),
            SourceError::Transient { .. }
        ));
        assert!(matches!(
            map_error(SmartMeError::Decode {
                reason: "x".to_string()
            }),
            SourceError::Transient { .. }
        ));
    }
}
