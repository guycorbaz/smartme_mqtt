//! Story 0.3 — the synthetic fixtures are well-formed and carry the expected smart-me
//! device shape, so parsing/oracle tests have a stable home before the real Epic 1 capture.
//!
//! # What this guard did not see until 2026-08-21 (issue #106)
//!
//! It asserted `device.get(key).is_some()` for the eight keys the client consumes.
//! **`serde_json` returns `Some(Value::Null)` for a key present with a null**, so a
//! fixture carrying `"ActivePower": null` passed this test while `Device` — which
//! requires all eight — cannot deserialize it at all. It also said nothing about
//! types: `"ActivePower": "0.018"` passed too.
//!
//! That is not a hypothetical shape. It is this repository's own recorded scar: the
//! API's description declares **six of the eight nullable**, `Device` requires all
//! eight, and `types.rs` carries the note about Guy's fourth meter — unplugged for
//! months — being exactly that shape. A fixture is supposed to be the thing that
//! catches a decode failure before the wire does.
//!
//! Both are checked now, and the mutations are kept as a fixture so the guard is
//! re-falsified on every CI pass rather than once by hand.

use std::path::Path;

use serde_json::{Value, json};

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// The eight fields `Device` consumes, and what each must BE.
///
/// Taken from `src/types.rs`, which is the type a real response is decoded through:
/// `Serial` is a JSON number, the two readings are floats, the rest are strings.
/// A fixture that disagrees with this table is a fixture that would not decode.
const REQUIRED: &[(&str, Shape)] = &[
    ("Id", Shape::Text),
    ("Name", Shape::Text),
    ("Serial", Shape::Number),
    ("ActivePower", Shape::Number),
    ("ActivePowerUnit", Shape::Text),
    ("CounterReading", Shape::Number),
    ("CounterReadingUnit", Shape::Text),
    ("ValueDate", Shape::Text),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Shape {
    Text,
    Number,
}

/// Judges one device object. Separate from the file walk so the mutations below go
/// through the same code the real fixture does.
fn device_shape_violations(device: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for (key, shape) in REQUIRED {
        match device.get(key) {
            None => out.push(format!("`{key}` is missing")),
            // The one this guard used to accept. A null loses nothing to the eye and
            // everything to `serde`: `Device` requires the field, so the whole
            // reading — energy index included — is lost to one absent momentary value.
            Some(Value::Null) => out.push(format!(
                "`{key}` is null; `Device` requires it, so a null here costs the whole \
                 reading rather than one field"
            )),
            Some(value) => {
                let ok = match shape {
                    Shape::Text => value.is_string(),
                    Shape::Number => value.is_number(),
                };
                if !ok {
                    out.push(format!(
                        "`{key}` is {value}, which is not the {shape:?} `Device` declares"
                    ));
                }
            }
        }
    }
    out
}

#[test]
fn smartme_sample_has_device_shape() {
    let raw = std::fs::read_to_string(fixtures().join("smartme_sample.json"))
        .expect("read smartme_sample.json");
    let v: Value = serde_json::from_str(&raw).expect("valid JSON");
    let devices = v.as_array().expect("payload is an array of devices");
    assert!(!devices.is_empty(), "at least one device");

    let mut offenders = Vec::new();
    for (index, device) in devices.iter().enumerate() {
        for violation in device_shape_violations(device) {
            offenders.push(format!("device[{index}]: {violation}"));
        }
    }
    assert!(
        offenders.is_empty(),
        "the fixture no longer has the shape `Device` decodes, so a test passing \
         against it proves nothing about a real response:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_shape_check_still_catches_a_null_and_a_wrong_type() {
    let sound = json!({
        "Id": "a1a1a1a1-b2b2-c3c3-d4d4-000000000001",
        "Name": "METER-A",
        "Serial": 30000001,
        "ActivePower": 0.018,
        "ActivePowerUnit": "kW",
        "CounterReading": 4843.822,
        "CounterReadingUnit": "kWh",
        "ValueDate": "2026-07-25T13:06:32.0500519Z"
    });
    assert!(
        device_shape_violations(&sound).is_empty(),
        "a device of the right shape must pass, or this guard proves nothing by \
         failing: {:?}",
        device_shape_violations(&sound)
    );

    let mut with_null = sound.clone();
    with_null["ActivePower"] = Value::Null;
    let said = device_shape_violations(&with_null).join("\n");
    assert!(
        said.contains("`ActivePower` is null"),
        "a null went unseen — `get(…).is_some()` is true for a null, which is exactly \
         how this guard was blind until 2026-08-21: {said}"
    );

    let mut wrong_type = sound.clone();
    wrong_type["ActivePower"] = json!("0.018");
    let said = device_shape_violations(&wrong_type).join("\n");
    assert!(
        said.contains("`ActivePower`") && said.contains("Number"),
        "a number sent as a string went unseen; `Device` declares `f64` and would \
         refuse it: {said}"
    );

    let mut missing = sound.clone();
    missing.as_object_mut().expect("object").remove("Serial");
    let said = device_shape_violations(&missing).join("\n");
    assert!(
        said.contains("`Serial` is missing"),
        "the case the first version DID catch must keep being caught: {said}"
    );
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
