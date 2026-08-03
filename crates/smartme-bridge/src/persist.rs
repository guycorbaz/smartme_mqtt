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

/// As [`persist_atomic`], but the file is created with `mode` **from the first
/// byte** rather than tightened afterwards.
///
/// The distinction is the whole point (ADR 0022). `File::create` applies the
/// umask, so a secret written by [`persist_atomic`] and `chmod`ed afterwards is
/// world-readable for the duration of the write — and the *temporary* file is
/// where the content actually lands, so tightening the final path would not even
/// cover it. The window is short. It is also entirely avoidable.
///
/// Unix only; on other platforms this is [`persist_atomic`] and the caller is
/// responsible for knowing that.
pub fn persist_atomic_with_mode<T: Serialize>(
    path: &Path,
    value: &T,
    #[cfg_attr(not(unix), allow(unused_variables))] mode: u32,
) -> io::Result<()> {
    let toml =
        toml::to_string_pretty(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    #[cfg(unix)]
    {
        write_atomic(path, toml.as_bytes(), Some(mode))
    }
    #[cfg(not(unix))]
    {
        write_atomic(path, toml.as_bytes(), None)
    }
}

/// Write `bytes` atomically to `path`: temp file + `fsync(file)` + `rename` + `fsync(dir)`.
pub fn persist_atomic_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes, None)
}

fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let tmp = tmp_path(path);

    {
        let mut f = match mode {
            #[cfg(unix)]
            Some(mode) => {
                use std::os::unix::fs::OpenOptionsExt as _;
                fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(mode)
                    .open(&tmp)?
            }
            #[cfg(not(unix))]
            Some(_) => File::create(&tmp)?,
            None => File::create(&tmp)?,
        };
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
