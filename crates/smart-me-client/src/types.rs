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
#[serde(rename_all = "PascalCase")]
pub struct Device {
    /// smart-me device UUID.
    pub id: String,
    /// Human-assigned device name.
    pub name: String,
    /// Physical device serial (a JSON number on the wire).
    pub serial: i64,
    /// Active power in `active_power_unit`.
    pub active_power: f64,
    /// Unit of `active_power` (observed: `"kW"`). Converted fail-closed in the
    /// bridge adapter (Story 1.7) — NEVER here.
    pub active_power_unit: String,
    /// Cumulative energy counter in `counter_reading_unit`.
    pub counter_reading: f64,
    /// Unit of `counter_reading` (observed: `"kWh"`).
    pub counter_reading_unit: String,
    /// Measurement timestamp, ISO-8601 UTC with `Z` and 7-digit fraction
    /// (observed: `2026-07-25T13:06:32.0500519Z`).
    pub value_date: String,
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
}
