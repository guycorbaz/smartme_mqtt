//! Story 1.13 — SIGTERM-NO-LIE: a clean stop must not leave a value on screen.
//!
//! The twin of `chaos_stale_on_death`, and deliberately not a duplicate of it.
//! That test kills the bridge outright and proves the BROKER's mechanism: the
//! will fires and the node goes down. This one proves the BRIDGE's own
//! mechanism: asked politely to stop, it publishes its death certificate itself,
//! before exiting.
//!
//! The distinction matters because the will alone is not enough. It is
//! serialised and handed to the broker inside the CONNECT packet, so it carries
//! the `bdSeq` and the timestamp of THAT connect — since Story 4.10 the driver
//! owns its reconnect loop and registers a fresh will, carrying a fresh session
//! number, on every CONNECT. It remains a fallback for the case where we die
//! without a chance to speak. A planned shutdown is exactly
//! the case where we DO have that chance, and an operator restarting the bridge
//! must not depend on the broker noticing a dropped socket.
//!
//! A review of Story 1.13 found that the explicit death never reached the wire
//! at all: `try_publish` only enqueued it and the EventLoop was never polled
//! again, so the certificate died in a channel — and the `disconnect()` that
//! followed would have told the broker to DISCARD the will too. Both mechanisms
//! would have failed together, and nothing in-process could have noticed. Hence
//! this test asserts from OUTSIDE, against a real broker, on the real binary.
//!
//! # How it tells the explicit death from the will
//!
//! On the wire the two are indistinguishable — same topic, same payload shape.
//! Arrival timing does not separate them either: the driver drops the socket
//! rather than sending DISCONNECT (deliberately, so the will survives as the
//! fallback), and a dropped socket makes the broker fire the will immediately,
//! not after the keep-alive.
//!
//! What separates them is **how many arrive**, and this reads no clock at all.
//!
//! A graceful stop produces **two** NDEATHs: the bridge publishes its own
//! certificate, then drops the socket rather than sending DISCONNECT —
//! deliberately, so the broker fires the will as the fallback (ADR 0011). **The
//! will can fire once.** So two certificates prove one of them was published by
//! the bridge; one proves the explicit path is broken.
//!
//! # Why this is not the timestamp comparison it used to be (Story 4.9)
//!
//! Until 2026-08-01 the discriminator was `death_stamp > birth_stamp`. It was
//! sound *because the will is serialised once, inside the first CONNECT*, so it
//! could never be stamped after any birth.
//!
//! **Story 4.10 rebuilds the will per CONNECT, and that silently inverts it.**
//! A will registered on a later connection is stamped later than the birth this
//! test captured on the first one, so a WILL would satisfy `death_stamp >
//! birth_stamp` and the test would go green on exactly the regression it exists
//! to catch. Nothing would announce it: no compile error, no edited assertion,
//! no failing run — the same shape as the `bdSeq` comparison that compared a
//! constant against itself. The comparison was therefore **removed**, not
//! supplemented; keeping both would leave a reader unable to tell which
//! assertion carries the proof.
//!
//! (An even earlier version compared the death against the instant the TEST sent
//! the signal. That spanned two clocks, and a backward NTP step during the run
//! would have let the will satisfy it. Recorded because the lesson generalises:
//! this discriminator has now been wrong twice, in two different ways, and both
//! times it still passed.)
//!
//! # What the count does NOT exclude
//!
//! A bridge that published its explicit death **twice** while the will never
//! fired would also produce two. That is a duplicate publish, not a missing one
//! — a different defect from the one under test, and excluded in practice by the
//! socket drop, which always fires the will. Stated rather than left for a reader
//! to assume the count proves more than it does.
//!
//! The buffered stream is also drained immediately before the signal, so a will
//! that fired during an earlier transport blip cannot be mistaken for the
//! shutdown's certificate.
//!
//! # Falsification record (Story 4.9, 2026-08-01) — copied from the runs
//!
//! Three experiments. The third is the one this story exists for: it shows the
//! OLD and NEW discriminators disagreeing about the SAME broken code.
//!
//! **Baseline, unmutated:** `test result: ok. 1 passed; 0 failed` (7.90 s).
//!
//! **Mutation A — the explicit death is not published** (`publish(&client,
//! publisher.will(…))` replaced by `let _ = publisher.will(…)` in
//! `mqtt_driver.rs`). RED:
//!
//! ```text
//! only ONE death certificate arrived after the SIGTERM. A graceful stop must
//! produce two — the bridge's own, and then the broker's will when the socket
//! drops. […] (bdSeq 42; SIGTERM sent at 1785593108125 …)
//! test result: FAILED. 0 passed; 1 failed
//! ```
//!
//! Restored byte-for-byte (`git diff` on the driver empty) and re-run GREEN:
//! `test result: ok. 1 passed; 0 failed` (7.51 s). The green direction is shown
//! deliberately — an expected-red never demonstrated green proves only that the
//! test can fail.
//!
//! **Mutation B — Story 4.10 simulated.** The will is stamped `now + 60_000`
//! (what a will rebuilt on a LATER CONNECT looks like relative to the first
//! birth) *and* the explicit death is not published, so only the will exists. A
//! temporary probe printed what the old assertion would have concluded:
//!
//! ```text
//! OLD-DISCRIMINATOR death_stamp(1785593432853) > birth_stamp(1785593372871) = true
//!                                                    <-- true means it would have PASSED
//! only ONE death certificate arrived after the SIGTERM. […]
//! test result: FAILED. 0 passed; 1 failed
//! ```
//!
//! **On identical broken code the old discriminator says pass and the new one
//! says fail.** That is the whole argument for this story, measured rather than
//! reasoned. The probe was removed before commit; the driver was restored
//! byte-for-byte and the suite re-run green (7.94 s).
//!
//! # What this test does NOT verify
//!
//! The AC's other clause — "an independent subscriber sees no fresh DDATA
//! survive" — is **vacuously satisfied here, not proven**. Two reasons, and the
//! honest word for both is unverified:
//!
//! 1. The smart-me cloud is unroutable in this scenario, so the poll task never
//!    obtains a reading and no DDATA is ever produced. There is nothing that
//!    could survive.
//! 2. Structurally, `supervisor::run` signals the death and then aborts the poll
//!    task, and the driver has already left its `select!` loop — so no path
//!    exists by which a reading could be published after the certificate through
//!    the only client that exists.
//!
//! The post-death drain below is therefore a guard against a future regression
//! in that ordering, not evidence about today's behaviour. Proving the clause
//! properly needs a TLS-terminating fake of the smart-me API (HTTPS is mandatory
//! and webpki rejects self-signed certs); recorded in deferred-work.md.

mod common;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::error::TryRecvError;

const SERIAL: &str = "30000001";
const NODE_ID: &str = "ChaosSigterm";

/// The session number seeded on disk before the bridge starts, and the number
/// the bridge must therefore adopt (`NodeSession::start` advances by one).
///
/// Seeding matters: with an empty state dir `load_bd_seq` falls back to the
/// `before_first` sentinel and every run births under the same low constant, so
/// pairing death to birth by `bdSeq` would compare a constant against itself and
/// an implementation that emitted a hard-coded number would pass.
const SEEDED_BD_SEQ: u8 = 41;
const EXPECTED_BD_SEQ: i64 = 42;

/// Kills the bridge if an assertion unwinds, so a failing test never leaves a
/// process running against a broker that is about to disappear.
struct Reaped(Child);

impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Removes the state directory on every path, including the failing ones.
///
/// This is a chaos test: failing IS the expected path when the bridge
/// misbehaves, so cleanup placed at the end of the body would run least often
/// when it matters most.
struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_millis() as u64
}

/// Explains a birth that never arrived, using what the child actually did.
///
/// Without this the test blocks for its full timeout and then blames the
/// subscriber, while the real cause — a rejected config, a missing environment
/// variable, an immediate abort — sits in a log nobody reads.
fn why_no_birth(what: &str, child: &mut Child, log: &Path) -> String {
    let state = match child.try_wait() {
        Ok(Some(status)) => format!("the bridge had ALREADY EXITED with {status}"),
        Ok(None) => "the bridge is still running".to_string(),
        Err(error) => format!("the bridge's status could not be read: {error}"),
    };
    let log = std::fs::read_to_string(log).unwrap_or_else(|_| "<no log captured>".to_string());
    let tail: Vec<&str> = log.lines().rev().take(20).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    format!(
        "no {what} reached the independent subscriber, and {state}.\n\
         Last lines of the bridge's own log:\n{}",
        tail.join("\n")
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn chaos_sigterm_no_lie() {
    let (_broker, port) = common::start_broker().await;
    run_case("127.0.0.1", port, "Site").await;
}

/// The same proof, against a broker that is not a container.
///
/// AR13 asked for the mechanism to be "confirmed against the author's broker",
/// and a containerised Mosquitto is a close but not identical proxy. This is
/// how that confirmation is taken.
///
/// **It is `#[ignore]`d and takes its target from the environment on purpose.**
/// A real broker is somebody's production: publishing a Sparkplug node onto one
/// makes every subscribed SCADA host discover it and persist it in its tag
/// tree, where it survives the test as a phantom device someone has to delete
/// by hand. That must never happen as a side effect of `cargo test`, so there
/// is no default target and no default group — both must be stated:
///
/// ```text
/// SMARTME_CHAOS_BROKER=host:1883 \
/// SMARTME_CHAOS_GROUP=ChaosTest \
///   cargo test -p smartme-bridge --test chaos_sigterm_no_lie -- --ignored --nocapture
/// ```
///
/// Choose a group that is obviously disposable. It is the top level of the
/// topic, so it is the folder the SCADA will create and the folder whoever
/// cleans up will look for.
///
/// # Cleaning up afterwards
///
/// The broker is left clean — the test asserts nothing is retained, so no
/// subscriber connecting later hears anything. The SCADA is not: Ignition's
/// MQTT Engine persists what it discovers, and the run leaves a tag folder
///
/// ```text
/// [MQTT Engine]Edge Nodes/<group>/<node>/<serial>   e.g. .../ChaosTest/ChaosSigterm/30000001
/// ```
///
/// Delete that folder in the Designer's tag browser — **only that folder**.
/// Deleting MQTT Engine tags also discards their alarm and history
/// configuration, and the real edge nodes live under the same parent. The node
/// never republishes, so a plain delete sticks; if tags reappear, Cirrus Link's
/// documented sequence is disable MQTT Engine, delete, re-enable.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "publishes to a real broker; needs SMARTME_CHAOS_BROKER and SMARTME_CHAOS_GROUP"]
async fn chaos_sigterm_no_lie_against_an_external_broker() {
    let target = std::env::var("SMARTME_CHAOS_BROKER")
        .expect("set SMARTME_CHAOS_BROKER=host:port — there is deliberately no default");
    let group = std::env::var("SMARTME_CHAOS_GROUP").expect(
        "set SMARTME_CHAOS_GROUP to a disposable group id — there is deliberately no default",
    );
    let (host, port) = target
        .rsplit_once(':')
        .expect("SMARTME_CHAOS_BROKER must be host:port");
    let port: u16 = port.parse().expect("the port must be a number");

    assert_ne!(
        group, "Site",
        "refusing to publish a test node into the default production group"
    );
    eprintln!("publishing a disposable Sparkplug node to {host}:{port} under group {group:?}");

    run_case(host, port, &group).await;
}

async fn run_case(broker_host: &str, port: u16, group: &str) {
    let mut seen = common::named_subscriber_on(broker_host, port, "independent-observer").await;

    let state_dir = std::env::temp_dir().join(format!("chaos_sigterm_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let state_dir = ScratchDir(state_dir);
    let bd_seq_path = state_dir.0.join("bdseq.toml");
    let log_path = state_dir.0.join("bridge.log");

    smartme_bridge::app::mqtt_driver::store_bd_seq(
        &bd_seq_path,
        sparkplug_b::BdSeq::new(SEEDED_BD_SEQ),
    )
    .expect("seed the session number so the bdSeq pairing below is not a tautology");

    let log = File::create(&log_path).expect("capture the bridge's log");
    let log_err = log.try_clone().expect("clone the log handle");

    // The real binary, in its own process. Every other test drives `app::run`
    // in-process, which no signal can reach; `main.rs → run() →
    // shutdown_signal()` is the path a SIGTERM actually travels and it is
    // covered nowhere else.
    // The bridge is configured by the FILE now (ADR 0023). Spawning it with the
    // old variables and no file would not fail — it would come up unconfigured
    // and never birth, so the SIGTERM this test measures would be delivered to a
    // process that had nothing to say goodbye about.
    common::write_config(&state_dir.0, group, NODE_ID, SERIAL, broker_host, port);

    let child = Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env("SMARTME_CLIENT_ID", "id")
        .env("SMARTME_CLIENT_SECRET", "secret")
        .env("SMARTME_STATE_DIR", state_dir.0.display().to_string())
        // reqwest honours the proxy environment by default. Inherited from a
        // developer's shell or a corporate runner, a proxy would route the
        // request to a host that ANSWERS, quietly falsifying the premise that
        // the cloud is silent — and a proxy named by hostname resolves on the
        // blocking pool, which `poll.abort()` cannot cancel and which the
        // runtime waits for at exit, stalling the shutdown this test measures.
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .expect("the bridge binary starts");
    let mut child = Reaped(child);

    // The node is on screen: a SCADA host now has something to show.
    let birth = match common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NBIRTH/")
    })
    .await
    {
        Some(birth) => birth,
        None => panic!("{}", why_no_birth("NBIRTH", &mut child.0, &log_path)),
    };
    let birth_bd_seq = birth.bd_seq().expect("a birth carries its session number");
    // The birth's TIMESTAMP is deliberately not read. It was the old
    // discriminator's other half, and Story 4.9 removed the comparison rather
    // than leaving a value in scope that a future edit could re-use for a test
    // that no longer holds. See the module docs.

    assert_eq!(
        birth_bd_seq, EXPECTED_BD_SEQ,
        "the bridge must adopt the session number persisted on disk ({SEEDED_BD_SEQ}, \
         advanced by one). Birthing under anything else means the seeded state was \
         not read, and the death-to-birth pairing below would compare two constants"
    );

    // And so is the device — this is the tag a stale value would be shown on.
    if common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/DBIRTH/") && s.topic.ends_with(SERIAL)
    })
    .await
    .is_none()
    {
        panic!("{}", why_no_birth("DBIRTH", &mut child.0, &log_path));
    }

    // Let the node live a moment before stopping it. This is what separates the
    // connect-time stamp the will carries from the shutdown-time stamp the
    // explicit death carries, and it is the whole margin the assertion below
    // rests on.
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Drain whatever is buffered. A transport blip earlier in the run would have
    // fired the will and left an NDEATH sitting in the observer's queue; without
    // this, the wait below would pop THAT one and blame the production code for
    // a certificate it did in fact publish afterwards.
    while seen.try_recv().is_ok() {}

    // Ask it to stop the way a container runtime does.
    let sigterm_at = now_ms();
    let signalled = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .expect("`kill` is available to send a real SIGTERM");
    assert!(
        signalled.success(),
        "SIGTERM was not delivered to the bridge"
    );

    let death = common::wait_for(&mut seen, Duration::from_secs(30), |s| {
        s.topic.contains("/NDEATH/")
    })
    .await
    .expect("a SIGTERM must produce a death certificate, not a silent exit");

    assert_eq!(
        death.bd_seq(),
        Some(birth_bd_seq),
        "the death must be attributable to the session that was born, or a \
         consumer pairing them by bdSeq will ignore it and keep the frozen \
         value on screen"
    );

    // COUNT the certificates. This is the discriminator, and it reads no clock.
    //
    // A graceful stop produces TWO NDEATHs: the bridge publishes its own, then
    // drops the socket rather than sending DISCONNECT — deliberately, so the
    // broker fires the will as well (ADR 0011). **The will can fire once.** So
    // two certificates prove one of them was published by the bridge, and one
    // certificate proves the explicit path is broken and the fallback is
    // carrying the graceful stop on its own.
    //
    // The second NDEATH may lag the first by a broker round-trip, so this waits
    // for it rather than draining immediately.
    let second = common::wait_for(&mut seen, Duration::from_secs(15), |s| {
        s.topic.contains("/NDEATH/")
    })
    .await;
    assert!(
        second.is_some(),
        "only ONE death certificate arrived after the SIGTERM. A graceful stop \
         must produce two — the bridge's own, and then the broker's will when \
         the socket drops. One means the explicit NDEATH never reached the wire \
         and the graceful path is leaning on a fallback that a graceful stop \
         must not need. (bdSeq {birth_bd_seq}; SIGTERM sent at {sigterm_at} by \
         the test's own clock, recorded for the log only — this assertion does \
         not compare timestamps.)"
    );
    assert_eq!(
        second.as_ref().and_then(|s| s.bd_seq()),
        Some(birth_bd_seq),
        "the second certificate belongs to a different session than the first; \
         both must carry the bdSeq that was born"
    );

    // Nothing may follow the certificate. A further NDEATH is not a violation:
    // the driver deliberately drops the socket rather than sending DISCONNECT,
    // so the broker's will fires too — the fallback doing its job, and a death
    // is never a lie. Anything else means a task outlived the announcement that
    // the node was gone.
    //
    // This runs BEFORE the exit is awaited, on purpose: after the process is
    // confirmed dead the check could not fail for any reason, which would make
    // it decoration rather than a guard.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mut survivors = Vec::new();
    loop {
        match seen.try_recv() {
            Ok(s) if s.topic.contains("/NDEATH/") => {}
            Ok(s) => survivors.push(s.topic),
            Err(TryRecvError::Empty) => break,
            // Silence and deafness are not the same result, and `try_recv().ok()`
            // would report them identically.
            Err(TryRecvError::Disconnected) => {
                panic!(
                    "the observer stopped listening; this check cannot tell silence from deafness"
                )
            }
        }
    }
    assert!(
        survivors.is_empty(),
        "the node published its own death and then kept talking; a consumer that \
         processes these in order sees the tag come back to life:\n{}",
        survivors.join("\n")
    );

    // A bridge that will not stop cannot be restarted honestly: the operator
    // eventually SIGKILLs it, and whatever it was going to say is lost.
    let mut status = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        if let Some(s) = child.0.try_wait().expect("the child's status is readable") {
            status = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let status = status.expect("the bridge must exit on SIGTERM rather than hang");
    assert!(
        status.success(),
        "a SIGTERM is a normal stop, not a crash; the bridge exited with {status}"
    );

    // And nothing may be waiting on the broker for the NEXT subscriber either.
    // Sparkplug forbids the retain flag on every message type, and this is why:
    // a retained BIRTH outlives the process, so every consumer connecting later
    // is handed a node that announces itself as online — one that exited
    // minutes ago. The bridge is gone and nothing else publishes here, so a
    // subscriber arriving now must hear silence.
    let mut latecomer = common::named_subscriber_on(broker_host, port, "late-observer").await;
    let retained = common::wait_for(&mut latecomer, Duration::from_secs(3), |_| true).await;
    assert!(
        retained.is_none(),
        "the broker replayed a retained message to a subscriber that connected \
         after the bridge was gone: {:?} — retained Sparkplug messages resurrect \
         dead nodes for every future consumer",
        retained.map(|s| s.topic)
    );
}
