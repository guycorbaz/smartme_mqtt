//! Atomic, crash-safe persistence primitive (Story 0.8).
//!
//! Shared by `bdSeq` (Epic 1) and config (Epic 5). The write sequence is
//! write-temp → `fsync(file)` → `rename` → `fsync(parent dir)`, so a crash never leaves
//! a torn file: a reader sees either the old value or the new one. Generic over
//! `T: Serialize` via TOML; carries no domain dependency, so any consumer can reuse it
//! without a forward dependency on a later epic.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Serialize `value` to TOML and write it atomically to `path`.
pub fn persist_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let toml =
        toml::to_string_pretty(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    persist_atomic_bytes(path, toml.as_bytes())
}

/// Write `bytes` atomically to `path`: temp file + `fsync(file)` + `rename` + `fsync(dir)`.
pub fn persist_atomic_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let tmp = tmp_path(path);

    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?; // fsync(file): data + metadata durable before the rename
    }

    fs::rename(&tmp, path)?; // atomic replace of the target

    // fsync(parent dir) so the rename entry itself survives a crash. Best-effort:
    // some filesystems/platforms don't support directory fsync.
    if let Some(dir) = parent
        && let Ok(d) = File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Load and TOML-deserialize a value previously written with [`persist_atomic`].
pub fn load<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let s = fs::read_to_string(path)?;
    toml::from_str(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}
