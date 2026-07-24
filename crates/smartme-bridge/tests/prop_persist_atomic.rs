//! Story 0.8 — atomicity / crash-safety of `persist_atomic`.

use serde::{Deserialize, Serialize};
use smartme_bridge::persist::{load, persist_atomic};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Sample {
    seq: u64,
    label: String,
}

fn tmpdir() -> PathBuf {
    let base = std::env::temp_dir().join("smartme_persist_test");
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn tmp_sibling(path: &std::path::Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}

#[test]
fn roundtrip_writes_and_reads_back() {
    let path = tmpdir().join("roundtrip.toml");
    let a = Sample {
        seq: 1,
        label: "a".into(),
    };
    persist_atomic(&path, &a).unwrap();
    let back: Sample = load(&path).unwrap();
    assert_eq!(a, back);
    std::fs::remove_file(&path).ok();
}

#[test]
fn overwrite_replaces_wholesale() {
    let path = tmpdir().join("overwrite.toml");
    persist_atomic(
        &path,
        &Sample {
            seq: 1,
            label: "old".into(),
        },
    )
    .unwrap();
    persist_atomic(
        &path,
        &Sample {
            seq: 2,
            label: "new".into(),
        },
    )
    .unwrap();
    assert_eq!(
        load::<Sample>(&path).unwrap(),
        Sample {
            seq: 2,
            label: "new".into()
        }
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn no_temp_file_left_behind() {
    let path = tmpdir().join("clean.toml");
    persist_atomic(
        &path,
        &Sample {
            seq: 7,
            label: "x".into(),
        },
    )
    .unwrap();
    assert!(
        !tmp_sibling(&path).exists(),
        "temp file was not consumed by the rename"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn interrupted_persist_leaves_old_value_intact() {
    // Crash-injection proxy: a persist that crashes *before* the rename leaves a stray
    // temp file; the committed target must still hold the OLD value, never a torn one.
    let path = tmpdir().join("crash.toml");
    persist_atomic(
        &path,
        &Sample {
            seq: 1,
            label: "committed".into(),
        },
    )
    .unwrap();

    // Bytes of a new value land in the temp file but the rename never happens.
    std::fs::write(tmp_sibling(&path), b"seq = 2\nlabel = \"torn\"\n").unwrap();

    // The committed value is unaffected by the abandoned temp file.
    assert_eq!(
        load::<Sample>(&path).unwrap(),
        Sample {
            seq: 1,
            label: "committed".into()
        }
    );

    // A subsequent successful persist replaces cleanly and clears the temp.
    persist_atomic(
        &path,
        &Sample {
            seq: 2,
            label: "clean".into(),
        },
    )
    .unwrap();
    assert_eq!(
        load::<Sample>(&path).unwrap(),
        Sample {
            seq: 2,
            label: "clean".into()
        }
    );
    assert!(!tmp_sibling(&path).exists());
    std::fs::remove_file(&path).ok();
}
