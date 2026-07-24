//! Story 0.3 — the synthetic fixtures are well-formed and carry the expected smart-me
//! device shape, so parsing/oracle tests have a stable home before the real Epic 1 capture.

use std::path::Path;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

#[test]
fn smartme_sample_has_device_shape() {
    let raw = std::fs::read_to_string(fixtures().join("smartme_sample.json"))
        .expect("read smartme_sample.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    let devices = v.as_array().expect("payload is an array of devices");
    assert!(!devices.is_empty(), "at least one device");
    for d in devices {
        for key in [
            "Id",
            "Name",
            "Serial",
            "ActivePower",
            "ActivePowerUnit",
            "CounterReading",
            "CounterReadingUnit",
            "ValueDate",
        ] {
            assert!(d.get(key).is_some(), "device is missing key `{key}`");
        }
    }
}

#[test]
fn http_header_slots_are_present() {
    let dir = fixtures().join("http_headers");
    for name in ["valid", "absent", "malformed", "negative_skew", "huge_skew"] {
        assert!(
            dir.join(format!("{name}.txt")).exists(),
            "missing HTTP-header fixture `{name}.txt`"
        );
    }
}
