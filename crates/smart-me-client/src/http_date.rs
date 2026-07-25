//! RFC 7231 `Date`-header parsing (Story 1.6).
//!
//! The `Date` header is an ORACLE INPUT, not transport plumbing: it is one side of
//! the freshness formula `age = Date − ValueDate` (ADR 0004). Absent or malformed
//! → `None` — never a substituted timestamp; the bridge's state machine draws the
//! conservative conclusion. Parsing is strict IMF-fixdate (the only form a
//! compliant server may send); the obsolete RFC 850 / asctime forms fail closed.

use crate::types::{calendar_day_valid, days_from_civil, fixed_digits};

/// Parses an IMF-fixdate header value ("Sat, 25 Jul 2026 13:06:33 GMT") to UTC
/// epoch-milliseconds. Strict: single-SP separated (RFC 7231 grammar), 2-digit
/// day / 4-digit year / 2-digit time fields all digits-only, calendar-valid date,
/// weekday consistent with the date, literal `GMT`, seconds 00–59 (a leap second
/// fails closed — one spurious STALE beats one fabricated millisecond).
pub fn parse_imf_fixdate(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split(' ').collect();
    let [wkday, day, mon, year, time, "GMT"] = parts.as_slice() else {
        return None;
    };
    let wkday_idx = match *wkday {
        // Index into the week with day 0 = Thursday (1970-01-01 was a Thursday),
        // matching `days_from_civil(...).rem_euclid(7)`.
        "Thu," => 0,
        "Fri," => 1,
        "Sat," => 2,
        "Sun," => 3,
        "Mon," => 4,
        "Tue," => 5,
        "Wed," => 6,
        _ => return None,
    };
    let day = fixed_digits(day, 2)?;
    let month = match *mon {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year = fixed_digits(year, 4)?;
    if !calendar_day_valid(year, month, day) {
        return None;
    }
    let mut hms = time.split(':');
    let h = fixed_digits(hms.next()?, 2)?;
    let m = fixed_digits(hms.next()?, 2)?;
    let sec = fixed_digits(hms.next()?, 2)?;
    if hms.next().is_some()
        || !(0..24).contains(&h)
        || !(0..60).contains(&m)
        || !(0..60).contains(&sec)
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if days.rem_euclid(7) != wkday_idx {
        // The named weekday contradicts the date: an internally inconsistent
        // header is not a trustworthy clock source.
        return None;
    }
    Some(days * 86_400_000 + (h * 3_600 + m * 60 + sec) * 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(name: &str) -> String {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/http_headers")
            .join(name);
        std::fs::read_to_string(p).expect("read header fixture")
    }

    /// The adapter-side extraction: lowercase-tolerant (HTTP/2), first `date:`.
    fn date_from(raw: &str) -> Option<i64> {
        let line = raw
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("date:"))?;
        parse_imf_fixdate(line.split_once(':')?.1.trim())
    }

    #[test]
    fn parses_the_real_captured_date() {
        let ms = date_from(&fixture("valid.txt")).expect("valid fixture parses");
        let expected =
            days_from_civil(2026, 7, 25) * 86_400_000 + (13 * 3_600 + 6 * 60 + 33) * 1_000;
        assert_eq!(ms, expected);
    }

    #[test]
    fn absent_and_malformed_fixtures_yield_none() {
        assert_eq!(date_from(&fixture("absent.txt")), None);
        assert_eq!(date_from(&fixture("malformed.txt")), None);
    }

    #[test]
    fn skew_fixtures_parse_to_their_documented_offsets() {
        let valid = date_from(&fixture("valid.txt")).unwrap();
        let neg = date_from(&fixture("negative_skew.txt")).unwrap();
        let huge = date_from(&fixture("huge_skew.txt")).unwrap();
        assert_eq!(valid - neg, 3_600_000, "negative_skew is 1 h earlier");
        assert_eq!(huge - valid, 365 * 86_400_000, "huge_skew is 1 y later");
    }

    #[test]
    fn strict_grammar_rejects_near_misses() {
        for bad in [
            "25 Jul 2026 13:06:33 GMT",         // missing weekday
            "Xxx, 25 Jul 2026 13:06:33 GMT",    // bogus weekday
            "Mon, 25 Jul 2026 13:06:33 GMT",    // weekday contradicts the date (it's a Sat)
            "Sat, 5 Jul 2026 13:06:33 GMT",     // 1-digit day
            "Sat, +5 Jul 2026 13:06:33 GMT",    // signed day token
            "Sat,  25 Jul 2026 13:06:33 GMT",   // double space (RFC says single SP)
            "Mon, 30 Feb 2026 13:06:33 GMT",    // calendar-invalid date
            "Sat, 25 Jul 2026 13:06:33 UTC",    // not GMT
            "Sat, 25 Jul 2026 13:06:60 GMT",    // leap second
            "Sat, 25 Jul 26 13:06:33 GMT",      // 2-digit year (RFC 850 style)
            "Saturday, 25-Jul-26 13:06:33 GMT", // RFC 850
            "Sat Jul 25 13:06:33 2026",         // asctime
        ] {
            assert_eq!(parse_imf_fixdate(bad), None, "should reject {bad:?}");
        }
    }
}
