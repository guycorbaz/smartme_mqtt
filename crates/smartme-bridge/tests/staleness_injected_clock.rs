//! Story 1.5 — the five captured header fixtures each map to their documented
//! verdict, driven through the pure `step` with an injected `FakeClock`.
//!
//! This is the localization twin of `chaos_stale_on_cloud_timeout` (Story 1.14):
//! same verdicts, zero network, zero real time. The fixtures are the REAL capture
//! (Story 1.1, api.smart-me.com via Cloudflare HTTP/2 — lowercase `date:`); this
//! test parses them like the adapter will, so the contract-of-record is exercised,
//! not paraphrased.

use std::path::{Path, PathBuf};

use smartme_bridge::core::clock::{Clock, FakeClock};
use smartme_bridge::core::{Policy, Reading, State, Tick};
use smartme_bridge::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};

/// METER-A's real ValueDate from the captured payload: 2026-07-25T13:06:32.0500519Z
/// (fractional .NET ticks truncated to milliseconds).
fn captured_value_date() -> UtcMillis {
    UtcMillis(utc_millis(2026, 7, 25, 13, 6, 32).0 + 50)
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crates dir")
        .join("smart-me-client/fixtures/http_headers")
}

/// Extracts and parses the `date:` header (case-insensitive — HTTP/2 lowercases)
/// as RFC 7231 IMF-fixdate. `None` for absent or unparseable — exactly the
/// adapter's contract (`Reading.http_date`).
fn http_date_from_fixture(name: &str) -> Option<UtcMillis> {
    let text = std::fs::read_to_string(fixtures_dir().join(name)).expect("read fixture");
    let line = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("date:"))?;
    parse_imf_fixdate(
        line.split_once(':')
            .expect("date line has a colon")
            .1
            .trim(),
    )
}

/// Minimal IMF-fixdate parser ("Sat, 25 Jul 2026 13:06:33 GMT"), test-only.
fn parse_imf_fixdate(s: &str) -> Option<UtcMillis> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let [_, day, mon, year, time, "GMT"] = parts.as_slice() else {
        return None;
    };
    let day: i64 = day.parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }
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
    let year: i64 = year.parse().ok()?;
    let mut hms = time.split(':');
    let h: i64 = hms.next()?.parse().ok()?;
    let m: i64 = hms.next()?.parse().ok()?;
    let sec: i64 = hms.next()?.parse().ok()?;
    if hms.next().is_some()
        || !(0..24).contains(&h)
        || !(0..60).contains(&m)
        || !(0..60).contains(&sec)
    {
        return None;
    }
    Some(utc_millis(year, month, day, h, m, sec))
}

/// Days-from-civil (Howard Hinnant) — enough calendar for a test.
fn utc_millis(y: i64, month: i64, day: i64, h: i64, min: i64, s: i64) -> UtcMillis {
    let y = if month <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    UtcMillis((days * 86_400 + h * 3_600 + min * 60 + s) * 1_000)
}

fn reading_with(http_date: Option<UtcMillis>) -> Reading {
    Reading {
        value: Measurement {
            meter: MeterId::new("METER-A"),
            serial: Serial::new("30000001"),
            power: Kw(0.018),
            energy: Kwh(4_843.822),
            value_date: captured_value_date(),
            quality: Quality::Good,
        },
        http_date,
    }
}

fn verdict_for(fixture: &str) -> (State, Quality) {
    let policy = Policy { max_age_ms: 90_000 };
    // The injected clock: a sane 2026 host wall time; the verdict must not depend
    // on real time in any way.
    let clock = FakeClock::new(utc_millis(2026, 7, 25, 13, 6, 40));
    let tick: Tick = Ok(reading_with(http_date_from_fixture(fixture)));
    policy.step(State::initial(), &tick, clock.wall())
}

#[test]
fn valid_fixture_is_the_only_fresh_one() {
    assert_eq!(verdict_for("valid.txt"), (State::Fresh, Quality::Good));
}

#[test]
fn absent_date_header_is_stale() {
    assert_eq!(verdict_for("absent.txt"), (State::Stale, Quality::Stale));
}

#[test]
fn malformed_date_header_is_stale() {
    assert_eq!(verdict_for("malformed.txt"), (State::Stale, Quality::Stale));
}

#[test]
fn negative_skew_is_stale() {
    assert_eq!(
        verdict_for("negative_skew.txt"),
        (State::Stale, Quality::Stale)
    );
}

#[test]
fn huge_skew_is_stale() {
    assert_eq!(verdict_for("huge_skew.txt"), (State::Stale, Quality::Stale));
}

#[test]
fn verdicts_hold_under_a_frozen_fake_clock() {
    // The FakeClock never advances unless told to: two evaluations are identical —
    // the oracle is a function, not a process.
    assert_eq!(verdict_for("valid.txt"), verdict_for("valid.txt"));
}
