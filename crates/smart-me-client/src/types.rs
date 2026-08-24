//! smart-me wire types and timestamp parsing (Story 1.6).
//!
//! The deserializer is shaped by the REAL captured payload (Story 1.1,
//! `fixtures/smartme_sample.json` — the contract-of-record), not by guesses:
//! `DeviceEnergyType` is an integer enum, `Serial` a JSON number, `Id` a UUID
//! string, and the payload carries many more fields than we consume — unknown
//! fields are ignored by design (the API may widen without breaking us).

use serde::Deserialize;

/// One device as returned by `GET /Devices` / `GET /Devices/{id}`.
///
/// Only the audited fields are deserialized; `ValueDate` stays a raw string here —
/// [`parse_value_date`] converts it, so a malformed timestamp is a visible `None`
/// at the conversion site, never a silently defaulted date.
#[derive(Debug, Clone, PartialEq, Deserialize)]
///
/// # Six of these eight are OPTIONAL, and requiring them cost whole readings
///
/// The API's own description — `docs/spec/smart-me-api/openapi-v1.json`, the
/// authority on presence and nullability — declares `Name`, `ActivePower`,
/// `ActivePowerUnit`, `CounterReading`, `CounterReadingUnit` and `ValueDate` as
/// nullable. Only `Id` and `Serial` are not.
///
/// This struct required all eight until 2026-08-24, so **one `null` failed the
/// whole deserialization** and the reading was lost entire — the energy index
/// included, read and convertible and thrown away with the rest ([#74]). That is
/// the failure story 2.5 repaired for units, arriving through the schema instead,
/// and it contradicts [ADR 0031], whose thesis is that a verdict belongs to a
/// metric.
///
/// The exposure stood unnoticed from story 1.6 in July until the description was
/// read in August — the reason `CLAUDE.md` now says to check both the description
/// and the fixtures, and to know which one is being quoted.
///
/// **The bridge's adapter already judges per field** (`map_device`'s
/// `SourceFaults`), so an absent number degrades ITS metric and lets its
/// neighbour through. What was missing was letting the reading get that far.
///
/// [#74]: https://github.com/guycorbaz/smartme_mqtt/issues/74
/// [ADR 0031]: ../../../docs/adr/0031-a-verdict-belongs-to-a-metric.md
#[serde(rename_all = "PascalCase")]
pub struct Device {
    /// smart-me device UUID.
    pub id: String,
    /// Human-assigned device name. **Nullable per the description.**
    pub name: Option<String>,
    /// Physical device serial (a JSON number on the wire). Non-nullable.
    pub serial: i64,
    /// Active power in `active_power_unit`. **Nullable per the description.**
    pub active_power: Option<f64>,
    /// Unit of `active_power` (observed: `"kW"`). Converted fail-closed in the
    /// bridge adapter (Story 1.7) — NEVER here. **Nullable per the description.**
    pub active_power_unit: Option<String>,
    /// Cumulative energy counter in `counter_reading_unit`. **Nullable per the
    /// description.**
    pub counter_reading: Option<f64>,
    /// Unit of `counter_reading` (observed: `"kWh"`). **Nullable per the
    /// description.**
    pub counter_reading_unit: Option<String>,
    /// Measurement timestamp, ISO-8601 UTC with `Z` and 7-digit fraction
    /// (observed: `2026-07-25T13:06:32.0500519Z`). **Nullable per the
    /// description.**
    pub value_date: Option<String>,
}

/// One device as it appears in the ACCOUNT LISTING (`GET /Devices`) — story 3.4.
///
/// # Deliberately not [`Device`], and the reason is [#74]
///
/// `Device` requires all eight fields it consumes, and the API's own description
/// declares six of them nullable — so deserializing the listing through it would
/// silently eject any real meter whose momentary reading carries a null, which
/// is a meter the operator cannot pick for a reason nobody is told. Guy's
/// fourth meter (`exterieur`, unplugged for months) is exactly that shape, and
/// it is a meter the operator must be able to SEE to decide about.
///
/// Discovery needs three facts and takes only those. Per the description:
/// `Id` is a non-nullable uuid, `Serial` a non-nullable int64, and `Name` is
/// NULLABLE — so it is an `Option` here, and a device without a name is shown
/// by its serial rather than given an invented one.
///
/// [#74]: https://github.com/guycorbaz/smartme_mqtt/issues/74
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeviceListing {
    /// smart-me device UUID — what `meters[].device_id` must hold.
    pub id: String,
    /// Human-assigned device name; the API declares it nullable and an absent
    /// JSON field is treated the same way (a listing is not a measurement, so
    /// the fail-closed rule of [`Device`] does not apply to a field whose only
    /// use is display).
    #[serde(default)]
    pub name: Option<String>,
    /// Physical device serial — what `meters[].serial` must hold, and what
    /// ADR 0029 verifies against every response.
    pub serial: i64,
}

/// Parses a smart-me `ValueDate` (ISO-8601, mandatory `Z`, optional fraction) to
/// UTC epoch-milliseconds. `None` on anything malformed — the caller decides the
/// conservative consequence; no substituted timestamp is ever produced here.
pub fn parse_value_date(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let year = fixed_digits(d.next()?, 4)?;
    let month = fixed_digits(d.next()?, 2)?;
    let day = fixed_digits(d.next()?, 2)?;
    if d.next().is_some() || !(1..=12).contains(&month) || !calendar_day_valid(year, month, day) {
        return None;
    }
    let (hms, frac) = match time.split_once('.') {
        Some((hms, frac)) => (hms, Some(frac)),
        None => (time, None),
    };
    let mut t = hms.split(':');
    let h = fixed_digits(t.next()?, 2)?;
    let mi = fixed_digits(t.next()?, 2)?;
    let sec = fixed_digits(t.next()?, 2)?;
    if t.next().is_some()
        || !(0..24).contains(&h)
        || !(0..60).contains(&mi)
        || !(0..60).contains(&sec)
    {
        return None;
    }
    let millis = match frac {
        None => 0,
        Some(f) if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) => return None,
        Some(f) => {
            // Truncate (never round) to milliseconds: 3 fractional digits.
            let padded = format!("{f:0<3}");
            padded.get(..3)?.parse::<i64>().ok()?
        }
    };
    Some(
        days_from_civil(year, month, day) * 86_400_000
            + (h * 3_600 + mi * 60 + sec) * 1_000
            + millis,
    )
}

/// Exactly `width` ASCII digits → value. Rejects signs, spaces, and any other
/// non-canonical token (`"+05"`, `"-026"`, `"7"` where `"07"` is required).
pub(crate) fn fixed_digits(s: &str, width: usize) -> Option<i64> {
    if s.len() != width || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// True when `day` exists in that month of that year (Gregorian, leap-aware) —
/// a Feb 30 must fail, not silently roll into March.
pub(crate) fn calendar_day_valid(y: i64, month: i64, day: i64) -> bool {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=max).contains(&day)
}

/// Days since 1970-01-01 (Howard Hinnant's days-from-civil).
pub(crate) fn days_from_civil(y: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_captured_value_date_shape() {
        // METER-A's real ValueDate; .0500519 truncates to 50 ms.
        let ms = parse_value_date("2026-07-25T13:06:32.0500519Z").expect("parses");
        let midnight = days_from_civil(2026, 7, 25) * 86_400_000;
        assert_eq!(ms, midnight + (13 * 3_600 + 6 * 60 + 32) * 1_000 + 50);
    }

    #[test]
    fn parses_without_fraction() {
        let ms = parse_value_date("2026-07-25T13:06:32Z").expect("parses");
        assert_eq!(ms % 1_000, 0);
    }

    #[test]
    fn rejects_malformed_value_dates() {
        for bad in [
            "2026-07-25T13:06:32",        // no Z: not UTC-marked
            "2026-07-25 13:06:32Z",       // no T
            "2026-13-25T13:06:32Z",       // month 13
            "2026-07-32T13:06:32Z",       // day 32
            "2026-02-30T13:06:32Z",       // calendar-invalid (Feb 30)
            "2026-7-25T13:06:32Z",        // non-padded month
            "2026-07-25T+3:06:32Z",       // signed hour token
            "-026-07-25T13:06:32Z",       // negative year
            "2026-07-25T24:00:00Z",       // hour 24
            "2026-07-25T13:06:60Z",       // leap second (fail closed)
            "2026-07-25T13:06:32.Z",      // empty fraction
            "2026-07-25T13:06:32.05a19Z", // non-digit fraction
            "garbage",
        ] {
            assert_eq!(parse_value_date(bad), None, "should reject {bad:?}");
        }
    }

    /// **Story 2.7 AC3 — a `ValueDate` that does not declare UTC is refused,
    /// and this is why refusing is right.**
    ///
    /// The bridge's freshness formula is `age = http_date − value_date`, two
    /// stamps subtracted as UTC epoch-milliseconds. A timestamp without its `Z`
    /// COULD be local time, and guessing a timezone for it would shift every
    /// reading by whole hours against a 90-second allowance — either every
    /// reading is Stale for no fault of the meter's, or an hours-old reading is
    /// published Fresh, which is the lie this project exists to prevent. An
    /// explicit offset is refused for the same reason seen from the other side:
    /// smart-me has only ever sent `Z` (the captured contract-of-record), so an
    /// offset appearing would mean the API's contract moved, and absorbing a
    /// contract change silently is what [`Cause::UnitNotRecognised`]'s exact
    /// matching already refuses for units.
    ///
    /// The rejects-malformed test above covers the no-`Z` case as grammar; this
    /// one exists because AC3 asks the assertion to say WHY, and to pin the
    /// shapes that specifically claim (or omit) a time zone.
    #[test]
    fn a_value_date_that_does_not_declare_utc_is_refused() {
        for undeclared in [
            "2026-07-25T13:06:32",       // no zone marker: could be local time
            "2026-07-25T13:06:32+00:00", // an offset, even the zero one
            "2026-07-25T13:06:32+02:00", // a real offset
            "2026-07-25T13:06:32-05:00",
            "2026-07-25T13:06:32z", // lowercase z is not the ISO-8601 UTC designator
        ] {
            assert_eq!(
                parse_value_date(undeclared),
                None,
                "{undeclared:?} does not explicitly declare UTC and must be \
                 refused: a guessed timezone shifts the age by whole hours \
                 against a 90-second allowance, silently"
            );
        }
    }

    /// **[#74] — a nullable field is no longer fatal, and what that costs is
    /// stated.**
    ///
    /// This test asserted the opposite until 2026-08-24: *"a payload missing a
    /// field we consume fails to parse and nothing is published at all"*, on the
    /// reasoning that a reading assembled from a payload we could not read would
    /// claim a measurement we do not have. **The reasoning was right and the scope
    /// was wrong.** It threw away the metrics that WERE readable with the one that
    /// was not — the energy index included, read and convertible — which is the
    /// failure story 2.5 repaired for units, arriving through the schema instead.
    ///
    /// Six of the eight fields are nullable per the API's own description, so this
    /// was not a hypothetical: it is the shape the wire is most likely to produce.
    ///
    /// # What this gives up, and it is worth naming
    ///
    /// `Option<T>` makes serde accept a field that is **absent** as well as one
    /// that is `null`, so a field the API REMOVED no longer stops anything. That
    /// detection is not lost, it moves: a metric whose value never arrives is
    /// permanently degraded with `value-unusable` on the wire and on the screens,
    /// which is visible — and it no longer costs the metric beside it.
    ///
    /// The two non-nullable fields keep the old guarantee, and the test below
    /// pins it.
    ///
    /// FALSIFIED 2026-08-24: making `active_power` a bare `f64` again — the state
    /// [#74] reported — turns the first assertion red, `null` failing the whole
    /// parse.
    #[test]
    fn a_nullable_field_costs_its_own_metric_and_not_the_reading() {
        // `null` where the description allows it: the reading still parses.
        let null_power = r#"{
            "Id": "1", "Name": "n", "Serial": 30000001,
            "ActivePower": null, "ActivePowerUnit": "kW",
            "CounterReading": 4843.822, "CounterReadingUnit": "kWh",
            "ValueDate": "2026-07-25T13:06:32.0500519Z"
        }"#;
        let parsed: Device = serde_json::from_str(null_power)
            .expect("a nullable field carrying null must not cost the reading");
        assert_eq!(
            parsed.active_power, None,
            "the absence is carried, not faked"
        );
        assert_eq!(
            parsed.counter_reading,
            Some(4843.822),
            "AND THE NEIGHBOUR SURVIVES: the energy index was readable and \
             convertible, and losing it to a null on the power was the whole of \
             [#74]"
        );

        // The two fields the description does NOT allow to be null keep the old
        // guarantee, and serde still names them.
        for (missing, body) in [
            (
                "Serial",
                r#"{"Id": "1", "Name": "n", "ActivePower": 0.7,
                    "ActivePowerUnit": "kW", "CounterReading": 1.0,
                    "CounterReadingUnit": "kWh", "ValueDate": "x"}"#,
            ),
            (
                "Id",
                r#"{"Name": "n", "Serial": 30000001, "ActivePower": 0.7,
                    "ActivePowerUnit": "kW", "CounterReading": 1.0,
                    "CounterReadingUnit": "kWh", "ValueDate": "x"}"#,
            ),
        ] {
            let parsed: Result<Device, _> = serde_json::from_str(body);
            let error = parsed.expect_err("a non-nullable field is still fatal");
            assert!(
                error.to_string().contains(missing),
                "and the error must name it — it is the only thing an operator has \
                 to go on. Got: {error}"
            );
        }
    }
}
