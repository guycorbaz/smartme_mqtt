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
    /// The logical meter this source serves (single-meter walking skeleton).
    meter: MeterId,
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
        device_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            clock,
            meter,
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

    async fn fetch_once(&mut self) -> Result<Reading, SmartMeError> {
        let token = self.token.as_ref().map(|t| &t.token);
        let capture = self.client.get_device(&self.device_id, token).await?;
        Ok(map_device(
            &capture.device,
            capture.http_date_ms,
            &self.meter,
        ))
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
            match self.fetch_once().await {
                Ok(reading) => Ok(reading),
                Err(SmartMeError::AuthRejected { .. }) if reusing_token => {
                    // ADR 0009: exactly one refresh + retry after a
                    // PREVIOUSLY-valid token — expiry can race the request in
                    // flight. A second rejection surfaces as the fatal it is.
                    self.token = None;
                    self.ensure_token().await?;
                    self.fetch_once().await.map_err(map_error)
                }
                Err(e) => Err(map_error(e)),
            }
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
fn map_device(device: &Device, http_date_ms: Option<i64>, meter: &MeterId) -> Reading {
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
    Reading {
        value: Measurement {
            meter: meter.clone(),
            serial: Serial::new(device.serial.to_string()),
            power,
            energy,
            value_date,
            quality,
        },
        http_date: http_date_ms.map(UtcMillis),
    }
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

    #[test]
    fn known_units_convert_and_stay_good() {
        let m = MeterId::new("m1");
        let r = map_device(&device("kW", "kWh", VD), Some(1_000), &m);
        assert_eq!(r.value.quality, Quality::Good);
        assert_eq!(r.value.power, Kw(0.018));
        assert_eq!(r.value.energy, Kwh(4_843.822));
        assert_eq!(r.http_date, Some(UtcMillis(1_000)));
        assert_eq!(r.value.serial.as_str(), "30000001");

        let w = map_device(&device("W", "Wh", VD), None, &m);
        // Same arithmetic as the converter — literal decimals differ in the
        // last binary digit.
        assert_eq!(w.value.power, Kw(0.018 / 1_000.0));
        assert_eq!(w.value.energy, Kwh(4_843.822 / 1_000.0));

        let mw = map_device(&device("MW", "MWh", VD), None, &m);
        assert_eq!(mw.value.power, Kw(18.0));
        assert_eq!(mw.value.energy, Kwh(4_843_822.0));
    }

    #[test]
    fn unknown_unit_fails_closed_to_bad_with_non_value_carrier() {
        let m = MeterId::new("m1");
        for (pu, eu) in [("kVA", "kWh"), ("kW", "MJ"), ("", ""), ("KW", "kWh")] {
            let r = map_device(&device(pu, eu, VD), Some(1_000), &m);
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
        let r = map_device(&device("KW", "kWh", VD), None, &m);
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
        let r = map_device(&device("kW", "kWh", "not-a-date"), Some(1_000), &m);
        assert_eq!(r.value.quality, Quality::Bad);
        assert_eq!(r.value.value_date, UtcMillis(0));
    }

    #[test]
    fn a_bad_reading_stays_bad_through_the_oracle() {
        // The adapter's Bad must not be relabeled Stale by the timestamp guards
        // (the epoch-pinned value_date would otherwise read as a huge age).
        use crate::core::state_machine::{Policy, State};
        let m = MeterId::new("m1");
        let r = map_device(&device("kVA", "kWh", "not-a-date"), Some(1_000), &m);
        let policy = Policy::DEFAULT;
        let (state, published) =
            policy.step(State::initial(), &Ok(r), UtcMillis(1_784_984_793_000));
        assert_eq!((state, published), (State::Stale, Quality::Bad));
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
