//! A cross-binary lock on the UI's default port.
//!
//! # Why this has to exist
//!
//! A first run has **no `config.toml` to read a port from**, so it listens on
//! [`ui::DEFAULT_PORT`](smartme_bridge::ui::DEFAULT_PORT) and nothing else can
//! be asked of it. That is correct in production — the container's internal port
//! is fixed and the mapping outside it is Docker's business — but it means every
//! test that exercises an unconfigured bridge wants the same port, and `cargo
//! test` runs test *binaries* concurrently, where a `Mutex` cannot reach.
//!
//! Without this, two such tests race and the loser reports "the UI never
//! answered" — a failure that looks exactly like the defect these tests exist to
//! catch. **A flake that impersonates a real bug is worse than no test.**
//!
//! Held by a lock *file* rather than by binding the port: the point is to
//! serialise, and a test that held the port itself would keep the bridge from
//! ever getting it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct PortLock(PathBuf);

impl PortLock {
    /// Wait for exclusive use of `port`, then take it.
    pub fn acquire(port: u16) -> Self {
        let path = std::env::temp_dir().join(format!("smartme_port_{port}.lock"));
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Self(path),
                Err(_) if Instant::now() < deadline => {
                    // A crashed run leaves the file behind; after the deadline
                    // take it anyway rather than hanging the suite forever.
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                    return Self(path);
                }
            }
        }
    }
}

impl Drop for PortLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
