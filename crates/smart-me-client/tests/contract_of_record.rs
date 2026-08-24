//! Story 1.6 — the captured fixture is the parsing contract-of-record.
//!
//! The client's deserializer and both timestamp parsers are pinned to the REAL
//! bytes captured in Story 1.1 (anonymized identities, everything else verbatim).
//! The transport itself is reqwest's tested territory; what is OURS — field
//! mapping, timestamp truncation, Date-header extraction — is proven here against
//! the same files the bridge's oracle tests consume (decision D7: no plaintext
//! HTTP mock — the client refuses non-TLS by design, so a mock would have to
//! disable the very guard under test).

use std::path::{Path, PathBuf};

use smart_me_client::{Device, parse_imf_fixdate, parse_value_date};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn devices() -> Vec<Device> {
    let raw = std::fs::read_to_string(fixtures().join("smartme_sample.json")).expect("read");
    serde_json::from_str(&raw).expect("the captured payload deserializes")
}

#[test]
fn all_captured_devices_deserialize() {
    let devs = devices();
    assert_eq!(devs.len(), 4, "the capture holds 4 devices");
}

fn meter_a(devs: &[Device]) -> &Device {
    devs.iter()
        .find(|d| d.name.as_deref() == Some("METER-A"))
        .expect("METER-A present in the capture")
}

#[test]
fn the_device_by_id_object_shape_deserializes_as_get_device_does() {
    // get_device parses a SINGLE object (`resp.json::<Device>()`), not the list:
    // this fixture is the real `GET /Devices/{id}` body (anonymized) parsed with
    // the exact same type parameter — closing the envelope gap flagged in review.
    let raw = std::fs::read_to_string(fixtures().join("smartme_device_by_id.json")).expect("read");
    let d: Device = serde_json::from_str(&raw).expect("single-object body deserializes");
    assert_eq!(d.name.as_deref(), Some("METER-A"));
    assert_eq!(
        d.value_date.as_deref(),
        Some("2026-07-25T13:06:32.0500519Z")
    );
}

#[test]
fn meter_a_fields_match_the_capture() {
    let devs = devices();
    let a = meter_a(&devs);
    assert_eq!(a.name.as_deref(), Some("METER-A"));
    assert_eq!(a.serial, 30_000_001);
    assert_eq!(a.id, "a1a1a1a1-b2b2-c3c3-d4d4-000000000001");
    assert_eq!(a.active_power_unit.as_deref(), Some("kW"));
    assert_eq!(a.counter_reading_unit.as_deref(), Some("kWh"));
    assert!(a.active_power.is_some_and(|p| p > 0.0));
    assert_eq!(
        a.value_date.as_deref(),
        Some("2026-07-25T13:06:32.0500519Z")
    );
}

#[test]
fn every_captured_value_date_parses() {
    for d in devices() {
        let ms = parse_value_date(d.value_date.as_deref().expect("the capture carries one"));
        assert!(ms.is_some(), "ValueDate {:?} must parse", d.value_date);
    }
}

#[test]
fn the_dead_meter_is_visible_in_the_capture() {
    // One device stopped reporting in April (96 days before capture) — the
    // natural STALE case. Its ValueDate must parse and sit far before METER-A's.
    let devs = devices();
    let ages: Vec<i64> = devs
        .iter()
        .map(|d| {
            parse_value_date(d.value_date.as_deref().expect("the capture carries one"))
                .expect("parses")
        })
        .collect();
    let newest = *ages.iter().max().expect("non-empty");
    let oldest = *ages.iter().min().expect("non-empty");
    assert!(
        newest - oldest > 90 * 86_400_000_i64,
        "the capture spans the 96-day-stale device"
    );
}

#[test]
fn value_date_and_captured_date_header_cohere() {
    // The cross-fixture check the whole freshness formula rests on: METER-A's
    // ValueDate vs the real valid.txt Date header → age ≈ +950 ms. (Couples the
    // payload and header fixtures BY DESIGN: they are one capture instant — if
    // ever re-captured, re-capture both together.)
    let devs = devices();
    let a = meter_a(&devs);
    let value_ms = parse_value_date(a.value_date.as_deref().expect("the capture carries one"))
        .expect("parses");
    let raw = std::fs::read_to_string(fixtures().join("http_headers/valid.txt")).expect("read");
    let date_line = raw
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("date:"))
        .expect("valid.txt has a date header");
    let http_ms = parse_imf_fixdate(date_line.split_once(':').expect("colon").1.trim())
        .expect("real Date parses");
    assert_eq!(http_ms - value_ms, 950, "age is the audited +950 ms");
}
