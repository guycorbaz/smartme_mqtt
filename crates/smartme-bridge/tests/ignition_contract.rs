//! Tier 3 — the deciding oracle, aimed at **the bridge's own bytes**.
//!
//! # Why this file exists next to `crates/sparkplug-b/tests/ignition_contract.rs`
//!
//! That one is older and still useful, but it stopped testing the product.
//! It scripts a session out of `sparkplug-b` primitives and publishes the
//! **specification's** quality codes; since [ADR 0012] the bridge publishes
//! Ignition's, which are different bytes. NFR17 asks whether *"Sparkplug B
//! output conforms to what Ignition MQTT Engine accepts"*, and the output that
//! matters is the product's.
//!
//! The drift is [#40]. The crate's test keeps a job — it is now the standing
//! external evidence that ADR 0012's deviation was **necessary**, because it
//! demonstrates the specified `Stale = 500` being displayed as `Good(500)` by a
//! real host. It is evidence about the **crate**. This file is evidence about
//! the **bridge**.
//!
//! # What makes this a Tier-3 gate rather than a test
//!
//! Every automated test in this workspace checks our bytes against our own
//! expectations. That cannot catch a code a host silently misreads — which is
//! exactly what happened with contract v1, where `Stale = 500` and `Bad = 0`
//! both displayed as **good** on a live Ignition. Only a human looking at a real
//! tag browser closes that gap, so the assertions here are printed instructions,
//! not `assert!`s.
//!
//! Two things ARE asserted mechanically, because a human cannot do them well:
//! the `bdSeq` before and after the rebirth is **read off the wire and printed**,
//! rather than the operator being asked whether it "looks the same"; and the
//! observer is connected **before** the bridge starts, so a birth that never
//! happened cannot be mistaken for one that scrolled past.
//!
//! # Running it
//!
//! ```text
//! SPARKPLUG_CONTRACT_BROKER=host:1883 \
//! SPARKPLUG_CONTRACT_GROUP=ContractV3 \
//!   cargo test -p smartme-bridge --test ignition_contract -- --ignored --nocapture
//! ```
//!
//! `--nocapture` is not optional: without it you see none of the prompts and the
//! gate appears to hang.
//!
//! # Why it refuses to guess its target
//!
//! No default broker, no default group, and it will not publish into a group
//! called `Site`. A Sparkplug host **persists what it discovers**: whatever group
//! you name becomes a folder in Ignition's tag tree that outlives this run and
//! has to be deleted by hand — and deleting MQTT Engine tags also discards their
//! alarm and history configuration. Publishing that into a production namespace
//! by accident is not recoverable by re-running anything.
//!
//! Clean-up is part of the procedure, not an afterthought — see
//! `docs/ignition-contract-runbook.md`.
//!
//! [ADR 0012]: ../../../docs/adr/0012-quality-codes-spec-versus-host.md
//! [#40]: https://github.com/guycorbaz/smartme_mqtt/issues/40

mod common;

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use smartme_bridge::app::mqtt_driver::{self, MqttConfig};
use smartme_bridge::core::channel::MeterUpdate;
use smartme_bridge::core::clock::{Clock, SystemClock};
use smartme_bridge::core::oracle::{Cause, Verdict};
use smartme_bridge::domain::{Kw, Kwh, Measurement, MeterId, Quality, Serial, UtcMillis};

use common::Seen;

const NODE_ID: &str = "ContractNodeV3";
const SERIAL: &str = "30000001";
const METER: &str = "contract-meter";

/// Values chosen to be unmistakable by eye: nothing round, nothing that could be
/// confused with a default, a placeholder, or one of Guy's real readings.
const POWER_FIRST: f64 = 1.234;
const POWER_SECOND: f64 = 2.345;
const ENERGY_FIRST: f64 = 5678.9;
const ENERGY_SECOND: f64 = 5679.1;

/// Removes the scratch state directory when the run ends, however it ends.
struct ScratchDir(std::path::PathBuf);
impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Prints what to check and waits. The pause is the point: an automated run
/// would prove only that the broker accepted the bytes.
///
/// `false_pass` is not decoration. `CLAUDE.md` requires every step of a
/// human-run gate to state what else could make it pass, because this gate
/// nearly returned a false pass once already: two of its five steps showed a
/// non-good quality for reasons unrelated to the property under test.
fn checkpoint(step: &str, look_for: &[&str], false_pass: &[&str]) {
    println!("\n──────────────────────────────────────────────────────────────");
    println!("  {step}");
    println!("──────────────────────────────────────────────────────────────");
    for item in look_for {
        println!("  [ ] {item}");
    }
    println!("\n  ⚠ This step also passes WRONGLY if:");
    for item in false_pass {
        println!("      · {item}");
    }
    print!("\n  Press Enter when you have checked the above (or Ctrl-C to abort)… ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

fn reading(power: f64, energy: f64, quality: Quality, now: UtcMillis) -> MeterUpdate {
    MeterUpdate::new(
        MeterId::new(METER),
        Measurement {
            meter: MeterId::new(METER),
            serial: Serial::new(SERIAL),
            power: Kw(power),
            energy: Kwh(energy),
            value_date: now,
            quality,
        },
        // Story 2.1: the gate publishes a verdict rather than a bare quality. The
        // cause is representative — this gate's step 4 asserts what a HOST
        // displays for a non-good quality, not which oracle refused.
        match quality {
            Quality::Good => Verdict::good(),
            Quality::Stale => Verdict::stale(Cause::ReadingTooOld),
            Quality::Bad => Verdict::bad(Cause::ValueUnusable),
        },
    )
}

/// Drains the observer into a shared transcript the run can interrogate.
fn collect(mut rx: mpsc::Receiver<Seen>) -> Arc<std::sync::Mutex<Vec<Seen>>> {
    let seen: Arc<std::sync::Mutex<Vec<Seen>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            sink.lock().expect("transcript lock").push(message);
        }
    });
    seen
}

/// The `bdSeq` of the most recent NBIRTH in the transcript, and how many NBIRTHs
/// have been seen. Both are printed rather than compared by eye.
fn births(seen: &Arc<std::sync::Mutex<Vec<Seen>>>) -> (usize, Option<i64>) {
    let t = seen.lock().expect("transcript lock");
    let births: Vec<&Seen> = t.iter().filter(|s| s.topic.contains("/NBIRTH/")).collect();
    (births.len(), births.last().and_then(|s| s.bd_seq()))
}

fn device_births(seen: &Arc<std::sync::Mutex<Vec<Seen>>>) -> usize {
    seen.lock()
        .expect("transcript lock")
        .iter()
        .filter(|s| s.topic.contains("/DBIRTH/"))
        .count()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual Tier-3 gate: drives the real bridge against a real broker for a human to inspect"]
async fn ignition_contract_v3() {
    // WITHOUT THIS, THE BRIDGE IS SILENT — and the silence looks like a pass.
    //
    // This gate drives `mqtt_driver::run` in-process. The only subscriber in the
    // crate is built in `main.rs`, which an integration test does not run, and
    // `tracing` with no subscriber discards every event regardless of `RUST_LOG`.
    // Three of step 5's checks are phrased "the bridge's log shows…" and none of
    // them could fire: the operator saw nothing and had no way to tell that from
    // a bridge that said nothing. Found on the 2026-08-03 run, [#44].
    //
    // INFO by default and NOT `fmt::init()`, for the reason `main.rs:135` gives:
    // `from_default_env()` defaults to ERROR, which drops the ignored-NCMD traces
    // and the near-miss WARNs that are exactly what step 5 asks the operator to
    // read. `RUST_LOG` still overrides.
    //
    // `try_init` rather than `init`: this file may not be the only test in the
    // process, and a second global subscriber is an error, not a reason to abort
    // a run a human is standing in front of.
    // FALSIFIED 2026-08-03, and the record is copied from the run, not written
    // from memory. Local `eclipse-mosquitto:2` on 127.0.0.1:18831, group
    // `FalsifyLocal`, stdin from /dev/null so the checkpoints fall through.
    // Counting `session born|no readable bdSeq|subscription granted`:
    //
    //     subscriber ENABLED  → 3
    //     subscriber DISABLED → 0
    //
    // The mutation was this block, made unreachable; nothing else changed. Both
    // levels step 5 depends on are covered by those three lines — `session born`
    // is INFO and `no readable bdSeq state` is WARN — so a `reason=NameOnlyNearly`
    // WARN will reach the operator too.
    //
    // A first count of 0 on the ENABLED run was a bad grep, not a bad fix: the
    // level is wrapped in ANSI colour codes, so a pattern of " INFO " with a
    // trailing space matches nothing. The tool's output described something other
    // than what was being measured — again.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with_test_writer()
        .try_init();

    let target = std::env::var("SPARKPLUG_CONTRACT_BROKER")
        .expect("set SPARKPLUG_CONTRACT_BROKER=host:port — there is deliberately no default");
    let group = std::env::var("SPARKPLUG_CONTRACT_GROUP").expect(
        "set SPARKPLUG_CONTRACT_GROUP to a disposable group — there is deliberately no default",
    );
    // Copied from the crate's gate rather than re-derived. See the module docs:
    // the folder outlives the run, and deleting it discards alarm and history
    // configuration for everything beneath it.
    assert_ne!(
        group, "Site",
        "refusing to publish a contract-test node into the default production group"
    );
    let (host, port) = target
        .rsplit_once(':')
        .expect("SPARKPLUG_CONTRACT_BROKER must be host:port");
    let port: u16 = port.parse().expect("the port must be a number");

    // The observer connects FIRST. A gate that subscribes after the bridge has
    // birthed cannot tell a birth that never happened from one it arrived too
    // late to see — and the Story 4.4 review found exactly that shape in the
    // will-versus-death discriminator.
    //
    // Bounded, because the helper is not. `named_subscriber_on` waits for a SubAck
    // and retries forever on error, so an unreachable broker makes this gate hang
    // with no output at all — a mistyped address would look like a slow start
    // rather than a wrong host. Found by running the gate against TEST-NET-1
    // while falsifying the `Site` guard.
    let transcript = collect(
        tokio::time::timeout(
            Duration::from_secs(20),
            common::named_subscriber_on(host, port, "tier3-observer-v3"),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "no MQTT connection to {host}:{port} within 20 s — the gate observes the wire \
                 before it publishes anything, so it stops here rather than starting a bridge \
                 nobody is watching. Check SPARKPLUG_CONTRACT_BROKER, the host's reachability, \
                 and that the broker accepts anonymous connections."
            )
        }),
    );

    let state_dir = std::env::temp_dir().join(format!("tier3_contract_{}", std::process::id()));
    std::fs::create_dir_all(&state_dir).expect("state dir");
    // NEVER the deployment's state directory: this run would consume a bdSeq the
    // production bridge is relying on, and hand it back a replayed session.
    let state_dir = ScratchDir(state_dir);

    let clock: Arc<dyn Clock + Send + Sync> = Arc::new(SystemClock::new());
    let node = sparkplug_b::EdgeNode::new(group.clone(), NODE_ID).expect("valid identifiers");
    let (tx, rx) = mpsc::channel(64);
    let (death_tx, death_rx) = oneshot::channel();

    let (_device_tx, device_rx) = tokio::sync::mpsc::channel(4);
    let driver = tokio::spawn(mqtt_driver::run(
        MqttConfig {
            client_id: format!("{NODE_ID}-tier3"),
            host: host.to_string(),
            port,
            keep_alive: Duration::from_secs(30),
            bd_seq_path: state_dir.0.join("bdseq.toml"),
            capacity: 64,
            death_flush: Duration::from_secs(2),
        },
        node,
        vec![Serial::new(SERIAL)],
        Arc::clone(&clock),
        rx,
        // AC4's reconfiguration channel. These tests never send on it; the
        // sender is kept alive so the driver's branch stays armed rather than
        // disarming on a dropped end.
        device_rx,
        death_rx,
    ));

    println!("\n  Publishing as group={group:?} node={NODE_ID:?} device={SERIAL:?}");
    println!("  Broker {host}:{port}");
    println!("  Clean-up afterwards: delete Edge Nodes/{group}/{NODE_ID} — and ONLY that folder.");

    // ------------------------------------------------------------ STEP 1 ---
    // The cold-start birth: the bridge announces itself before it has any
    // reading to announce.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (n, bd_at_birth) = births(&transcript);
    println!("\n  [wire] NBIRTHs seen so far: {n}; bdSeq = {bd_at_birth:?}");

    checkpoint(
        "STEP 1 — cold start: the node appears, and its tags are honestly STALE",
        &[
            "The tag folder Edge Nodes/<group>/ContractNodeV3 exists",
            "Contract/Version is present and reads 3 (NOT 2 — v3 added the rebirth metric)",
            "Node Control/Rebirth is present, Boolean, and reads false",
            "Power and Energy exist for device 30000001",
            "Their quality is NOT good — nothing has been read yet, and the bridge says so",
        ],
        &[
            "the folder is left over from an EARLIER run — check the timestamps, and that \
             you deleted it last time",
            "the tags read stale because Ignition has not connected yet, rather than because \
             the bridge said so — confirm the node shows as online",
            "Contract/Version reads 2 because you are looking at a node published by the \
             OLD crate-level gate, which declares no rebirth metric at all",
        ],
    );

    // ------------------------------------------------------------ STEP 2 ---
    tx.send(reading(
        POWER_FIRST,
        ENERGY_FIRST,
        Quality::Good,
        clock.wall(),
    ))
    .await
    .expect("driver alive");
    tokio::time::sleep(Duration::from_secs(2)).await;

    checkpoint(
        "STEP 2 — a good reading arrives",
        &[
            "Power reads 1.234 kW and Energy reads 5678.9 kWh",
            "Both qualities are GOOD",
            "The engineering units are shown (kW, kWh)",
        ],
        &[
            "you are reading the values from STEP 1 — those were null, so a number here is \
             genuinely new, but check the timestamp moved",
            "Ignition is showing its own 'last known good' rather than a fresh value",
        ],
    );

    // ------------------------------------------------------------ STEP 3 ---
    tx.send(reading(
        POWER_SECOND,
        ENERGY_SECOND,
        Quality::Good,
        clock.wall(),
    ))
    .await
    .expect("driver alive");
    tokio::time::sleep(Duration::from_secs(2)).await;

    checkpoint(
        "STEP 3 — the value updates in place",
        &[
            "Power now reads 2.345 kW and Energy 5679.1 kWh",
            "The values CHANGED rather than a second set of tags appearing",
            "Quality is still GOOD",
        ],
        &[
            "a second device folder appeared and you are reading the new one — the topic \
             grammar is keyed on the serial, so a duplicate means the serial changed",
            "the timestamp did not move, and you are looking at STEP 2's value",
        ],
    );

    // ------------------------------------------------------------ STEP 4 ---
    // THE STEP THIS WHOLE FILE EXISTS FOR.
    //
    // It exercises `ignition_quality_code`, which is the entire reason ADR 0012
    // exists and the one thing no automated test in this workspace can check:
    // whether the host AGREES that this code means "not good". Contract v1
    // published the specification's `Stale = 500` and Ignition displayed
    // `Good(500)` — every non-good quality failed toward good, which is the exact
    // silent lie this project exists to prevent.
    //
    // The crate-level gate still publishes 500 here, deliberately, and its step 4
    // now expects `Good(500)`. If THIS step also showed good, the deviation would
    // have stopped working.
    tx.send(reading(
        POWER_SECOND,
        ENERGY_SECOND,
        Quality::Stale,
        clock.wall(),
    ))
    .await
    .expect("driver alive");
    tokio::time::sleep(Duration::from_secs(2)).await;

    checkpoint(
        "STEP 4 — an honest STALE, in Ignition's own encoding (ADR 0012)",
        &[
            "Both tags now show a NON-GOOD quality — Ignition renders it as Bad_Stale",
            "The VALUES are unchanged (2.345 / 5679.1): the bridge reports the last known \
             reading and marks it untrustworthy, rather than blanking it",
            "The quality overlay is visible in the tag browser without hovering",
        ],
        &[
            "★ the tag is stale because the BRIDGE DIED and Ignition applied its own \
             transport-level staleness — check the node still shows ONLINE. This is the \
             false pass that nearly slipped through on the v2 run",
            "the tag is stale because Ignition's own tag group is not polling",
            "you are reading a quality of Good(500): that is the SPECIFICATION's stale code \
             being misread as good, and means you are looking at the crate-level gate's \
             node, not this one",
            "the value blanked instead of freezing — that is a different (and wrong) \
             behaviour that also looks 'not good'",
        ],
    );

    // ------------------------------------------------------------ STEP 5 ---
    // AC3, AC4, AC5. The one thing no test in this repository can do: prove the
    // bridge answers IGNITION, not us.
    let (births_before, bd_before) = births(&transcript);
    let dbirths_before = device_births(&transcript);
    println!(
        "\n  [wire] before the rebirth: NBIRTH count = {births_before}, bdSeq = {bd_before:?}, DBIRTH count = {dbirths_before}"
    );
    println!("  ── Now trigger a rebirth FROM IGNITION. Do not publish it yourself. ──");
    println!("     In Designer: the tag Edge Nodes/{group}/{NODE_ID}/Node Control/Rebirth,");
    println!("     written to `true`. Record WHERE you found it — the next person will not.");
    println!("     If no such control exists, that ABSENCE is the measurement (AC4): it would");
    println!("     mean MQTT Engine offers the control only for a node that declared the");
    println!("     metric, and that ADR 0016 described a flow which had never occurred.");

    checkpoint(
        "STEP 5a — issue the rebirth from Ignition, then press Enter",
        &[
            "You found a Rebirth control — write down exactly where it appears",
            "You wrote `true` to it from the Designer, not from a script or mosquitto_pub",
        ],
        &[
            "you published the rebirth yourself — that proves the bridge answers US, which \
             every automated test already proves, and is not what this gate is for",
            "the control exists but writing to it silently did nothing — check Ignition's \
             own logs for the outgoing NCMD",
        ],
    );

    tokio::time::sleep(Duration::from_secs(3)).await;
    let (births_after, bd_after) = births(&transcript);
    let dbirths_after = device_births(&transcript);
    println!(
        "\n  [wire] after the rebirth:  NBIRTH count = {births_after}, bdSeq = {bd_after:?}, DBIRTH count = {dbirths_after}"
    );
    println!(
        "  [wire] NBIRTHs gained: {}   DBIRTHs gained: {}",
        births_after.saturating_sub(births_before),
        dbirths_after.saturating_sub(dbirths_before)
    );
    println!(
        "  [wire] bdSeq {} — the specification requires it UNCHANGED across a rebirth",
        match (bd_before, bd_after) {
            (Some(a), Some(b)) if a == b => format!("unchanged at {a}  ✓"),
            (Some(a), Some(b)) => format!("CHANGED {a} → {b}  ✗ THIS IS A DEFECT"),
            _ => "could not be read from the wire".to_string(),
        }
    );

    checkpoint(
        "STEP 5b — the answer, judged on the printed numbers above",
        &[
            "Exactly one NBIRTH was gained, and one DBIRTH per meter",
            "bdSeq is reported unchanged above — read the line, do not judge by eye",
            "The bridge's log shows BOTH 'Rebirth Request accepted' AND 'node re-announced \
             on a Rebirth Request' — they are different events, and the first fires before \
             the birth is attempted",
            "Ignition's Node Info shows its Rebirth counter incremented",
        ],
        &[
            "a RECONNECT produced the birth, not the rebirth. NOTE, since Story 4.10 this \
             no longer applies to THIS bridge: one CONNECT is one bdSeq, so a reconnect \
             mints a NEW number and the verdict printed above already excludes it. The \
             warning is kept because it applies to any build predating 4.10, and because \
             a bdSeq line reading 'could not be read from the wire' leaves you with no \
             discriminator at all — in that case, and only then, check the log for a \
             connection event",
            "★ a RETAINED NCMD was replayed at subscribe time rather than a request anyone \
             sent (ADR 0017) — the bridge now refuses those, and logs reason=Retained",
            "the birth was already in flight from an earlier step and you counted it twice",
            "no birth followed and you concluded the bridge is broken WITHOUT looking for \
             the near-miss WARN: reason=NameOnlyNearly means Engine sent a different \
             spelling (the norm contradicts itself — Sparkplug_5:950 says 'Node \
             Control/Refresh' where every tck-id says 'Rebirth'); reason=ValueNotTrue means \
             a different encoding. Each has a different repair",
        ],
    );

    println!("\n  RECORD FOR THE RUN TABLE (AC5, AC8):");
    println!("    · the metric NAME Engine sent, exactly as received:");
    println!("    · its datatype and value:");
    println!("    · where the Rebirth control appears in Designer:");
    println!("    · Ignition version, and the MQTT ENGINE MODULE version:");

    // ------------------------------------------------------------ STEP 6 ---
    let _ = death_tx.send(());
    let _ = tokio::time::timeout(Duration::from_secs(10), driver).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    checkpoint(
        "STEP 6 — a planned stop is announced, not merely noticed",
        &[
            "The node shows OFFLINE in Ignition",
            "Every tag under it is marked not-good",
            "The bridge logged an explicit death BEFORE the connection dropped (ADR 0011 \
             requires both: the explicit NDEATH and the broker's will)",
            "WRITE DOWN, from Node Info, before reading any further: the Death Count, and \
             the DATE AND TIME on Offline DateTime — both fields, the date as well as the \
             time. Record what you see; do not compare it to anything yet",
        ],
        &[
            "Ignition marked it offline on a keep-alive timeout rather than on a death \
             certificate — that takes ~30 s, so if it went offline instantly you saw a real \
             death; if it lagged, you did not",
            "you are watching a different node go offline",
        ],
    );

    // Printed AFTER the checkpoint above, deliberately. What the field is for is
    // not a secret; announcing it in the same breath as asking for the reading is
    // what cost the 2026-08-03 run its measurement. Ask, then look, then compare.
    println!("\n  Now — and not before — what that reading is for:");
    println!("    The two deaths are ~2 s apart: the explicit NDEATH first, the will second.");
    println!("    Which timestamp the host kept says whether ADR 0011's claimed benefit");
    println!("    — 'the explicit certificate is immediate' — is observable from the host");
    println!("    side at all. Add your figures to the run table either way.");
    println!("    If they match a value already written in the runbook to the second,");
    println!("    check the DATE before recording: that has happened once.");

    println!("\n  ────────────────────────────────────────────────────────────");
    println!("  CLEAN-UP IS PART OF THE PROCEDURE");
    println!("  Delete Edge Nodes/{group}/{NODE_ID} under the MQTT Engine provider,");
    println!("  and ONLY that folder. Ignition persists what it discovers.");
    println!("  ────────────────────────────────────────────────────────────");
}
