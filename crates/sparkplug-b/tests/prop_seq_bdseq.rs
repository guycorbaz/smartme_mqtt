//! Story 1.8 — properties of the sequence numbering, checked exhaustively.
//!
//! The domain of both counters is 256 values, so these are not sampled
//! properties: every value and every wrap point is exercised. A sampled test
//! could miss exactly the boundary that matters (255 → 0), which is the one a
//! consumer's gap detection depends on.

use prost::Message;
use sparkplug_b::protobuf::{Payload, payload};
use sparkplug_b::{BD_SEQ_METRIC, BdSeq, Metric, MetricValue, NodeSession, Quality, SeqCounter};

/// Every sequence number is in range, and the successor of 255 is 0 — never 256.
#[test]
fn prop_seq_stays_in_range_and_wraps_at_the_boundary() {
    let mut seq = SeqCounter::new();
    // Three full laps: the wrap must be reproducible, not a one-off.
    for lap in 0..3 {
        for expected in 0..=255_u64 {
            let got = seq.take();
            assert_eq!(got, expected, "lap {lap}: numbering must be consecutive");
            assert!(got <= 255, "a sequence number never leaves 0..=255");
        }
    }
    assert_eq!(seq.peek(), 0, "after three laps we are back at the start");
}

/// The successor relation holds from every starting point: `next = (n + 1) % 256`.
#[test]
fn prop_seq_successor_is_modular_from_every_start() {
    for start in 0..=255_u64 {
        let mut seq = SeqCounter::new();
        for _ in 0..start {
            seq.take();
        }
        let n = seq.take();
        assert_eq!(n, start);
        assert_eq!(seq.take(), (start + 1) % 256, "successor of {start}");
    }
}

/// The same wrap, observed through the PUBLISHED payloads rather than the raw
/// counter — this is the form the acceptance criterion is written in.
#[test]
fn prop_published_messages_wrap_255_to_0() {
    let (mut live, birth) = NodeSession::start(BdSeq::before_first()).birth(0, vec![]);
    assert_eq!(birth.seq, Some(0), "the BIRTH opens the numbering at 0");
    // Messages 1..=255 fill the lap...
    for expected in 1..=255_u64 {
        assert_eq!(live.data(expected, vec![]).seq, Some(expected));
    }
    // ...and the next one wraps to 0 without a rebirth.
    assert_eq!(
        live.data(256, vec![]).seq,
        Some(0),
        "the 257th message of a session wraps to 0"
    );
    assert_eq!(live.data(257, vec![]).seq, Some(1));
}

/// A BIRTH always restarts at 0, whatever the counter had reached — that is what
/// tells a consumer "numbering restarted", not "you missed 200 messages".
#[test]
fn prop_rebirth_always_restarts_numbering_at_zero() {
    for messages_before in [0_u32, 1, 42, 255, 256, 300] {
        let (mut live, _) = NodeSession::start(BdSeq::before_first()).birth(0, vec![]);
        for _ in 0..messages_before {
            let _ = live.data(0, vec![]);
        }
        let rebirth = live.rebirth(1_000, vec![]);
        assert_eq!(
            rebirth.seq,
            Some(0),
            "after {messages_before} messages a BIRTH still carries seq 0"
        );
        assert_eq!(live.data(1_001, vec![]).seq, Some(1));
    }
}

/// `bdSeq` advances by exactly one per session and wraps after 255 — so a node
/// that reconnects 256 times returns to where it started, with no gap.
#[test]
fn prop_bdseq_is_continuous_across_sessions() {
    let mut bd = BdSeq::before_first();
    for step in 0..=600_u32 {
        let expected = (step % 256) as u8;
        assert_eq!(bd.value(), expected, "session {step}");
        let session = NodeSession::start(bd);
        assert_eq!(
            session.bd_seq().value(),
            ((step + 1) % 256) as u8,
            "starting a session advances bdSeq exactly once"
        );
        bd = session.bd_seq();
    }
}

/// The will registered before connecting, the BIRTH, and the DEATH all carry the
/// same `bdSeq` — the pairing a consumer uses to ignore a death belonging to a
/// session that has already been superseded.
#[test]
fn prop_will_birth_and_death_agree_on_bdseq_for_every_session_number() {
    for start in 0..=255_u8 {
        let session = NodeSession::start(BdSeq::new(start));
        let will = session.will(500);
        let (live, birth) = session.birth(1_000, vec![sample_metric()]);
        let death = live.death(2_000);

        let will_bd = bd_seq_of(&will).expect("the will carries bdSeq");
        let birth_bd = bd_seq_of(&birth).expect("BIRTH carries bdSeq");
        let death_bd = bd_seq_of(&death).expect("DEATH carries bdSeq");
        assert_eq!(will_bd, birth_bd, "session starting from {start}");
        assert_eq!(birth_bd, death_bd, "session starting from {start}");
        assert_eq!(
            birth_bd,
            i64::from(start.wrapping_add(1)),
            "the published number is the session's own"
        );
        assert_eq!(will.seq, None, "a DEATH is never numbered");
        assert_eq!(death.seq, None);
    }
}

/// Restart continuity: persisting the number and resuming from it produces the
/// next session, never a replay of one a consumer has already seen.
#[test]
fn prop_bdseq_survives_a_restart_without_replaying_a_number() {
    for start in 0..=255_u8 {
        let live = NodeSession::start(BdSeq::new(start));
        let persisted = live.bd_seq().value();

        // A restart reads the persisted number and starts the NEXT session.
        let after_restart = NodeSession::start(BdSeq::new(persisted));
        assert_ne!(
            after_restart.bd_seq().value(),
            persisted,
            "a restart must not replay the number a consumer already saw"
        );
        assert_eq!(
            after_restart.bd_seq().value(),
            persisted.wrapping_add(1),
            "and it must not skip one either"
        );
    }
}

/// Whatever the numbering, the payload stays wire-valid and decodes back.
#[test]
fn prop_every_numbered_payload_round_trips() {
    let (mut live, _) = NodeSession::start(BdSeq::before_first()).birth(0, vec![sample_metric()]);
    for i in 0..300_u64 {
        let p = live.data(i, vec![sample_metric()]);
        let bytes = sparkplug_b::encode(&p);
        let decoded = Payload::decode(bytes.as_slice()).expect("payload is valid protobuf");
        assert_eq!(decoded.seq, p.seq, "message {i}");
        assert_eq!(decoded, p);
    }
}

fn sample_metric() -> Metric {
    Metric::new("Counter", MetricValue::Double(1.5), 1_000)
        .with_quality(Quality::Good)
        .with_engineering_unit("kWh")
}

/// Reads the `bdSeq` metric out of a payload, whatever its position.
fn bd_seq_of(p: &Payload) -> Option<i64> {
    let m = p
        .metrics
        .iter()
        .find(|m| m.name.as_deref() == Some(BD_SEQ_METRIC))?;
    match m.value {
        Some(payload::metric::Value::LongValue(v)) => Some(v as i64),
        _ => None,
    }
}
