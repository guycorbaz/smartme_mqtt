//! The poll+publish task (Story 1.11).
//!
//! One meter, one loop: heartbeat, fetch, JUDGE, forward. The judging is the
//! pure [`Policy::step`] — no truth is decided in this `async fn`, it only
//! carries data to and from the function that decides.
//!
//! The state machine lives ENTIRELY here and never crosses into the mqtt task,
//! which knows only connection birth and death.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::core::channel::MeterUpdate;
use crate::core::clock::{Clock, MonotonicMs};
use crate::core::source::{Source, SourceError, Tick};
use crate::core::state_machine::{Policy, State};
use crate::domain::MeterId;

/// How the loop is paced and how long a single fetch may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollConfig {
    /// Delay between the start of one iteration and the next.
    pub interval: Duration,
    /// Per-fetch deadline. Beyond it the fetch is abandoned and the tick counts
    /// as [`SourceError::Timeout`] — the cloud going silent must not wedge the
    /// loop, because a wedged loop stops publishing STALE and starts lying by
    /// omission.
    pub fetch_timeout: Duration,
}

/// The liveness heartbeat: the monotonic instant at the top of the last loop
/// iteration.
///
/// Written before the network call, so a fetch that hangs forever leaves the
/// heartbeat visibly old — that is what makes a wedge detectable from outside
/// (the health check, Story 1.13/Epic 7). A heartbeat written AFTER the call
/// would look healthy exactly when it is not.
#[derive(Debug, Clone)]
pub struct LastLoopTick(Arc<std::sync::atomic::AtomicI64>);

impl LastLoopTick {
    /// A heartbeat that has never ticked.
    pub fn new() -> Self {
        Self(Arc::new(std::sync::atomic::AtomicI64::new(i64::MIN)))
    }

    /// Records that an iteration has just started.
    pub fn touch(&self, now: MonotonicMs) {
        self.0.store(now.0, std::sync::atomic::Ordering::Relaxed);
    }

    /// The last recorded instant, or `None` if the loop has never run.
    pub fn last(&self) -> Option<MonotonicMs> {
        match self.0.load(std::sync::atomic::Ordering::Relaxed) {
            i64::MIN => None,
            v => Some(MonotonicMs(v)),
        }
    }
}

impl Default for LastLoopTick {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything one iteration needs that does not change between iterations.
pub struct Context<'a> {
    /// The meter being polled.
    pub meter: &'a MeterId,
    /// The injected clock.
    pub clock: &'a (dyn Clock + Send + Sync),
    /// The staleness policy.
    pub policy: Policy,
    /// Loop pacing and the fetch deadline.
    pub config: PollConfig,
    /// The liveness heartbeat.
    pub heartbeat: &'a LastLoopTick,
    /// Where judged readings go.
    pub outbox: &'a mpsc::Sender<MeterUpdate>,
}

/// Runs one iteration: heartbeat, fetch (bounded), judge, forward.
///
/// Split out of the loop so a test can drive exactly one step without a timer.
/// Returns the state to carry into the next iteration.
pub async fn step_once<S: Source + Send>(
    ctx: &Context<'_>,
    source: &mut S,
    previous: State,
) -> State {
    let Context {
        meter,
        clock,
        policy,
        config,
        heartbeat,
        outbox,
    } = ctx;
    let (policy, config) = (*policy, *config);
    // Heartbeat FIRST: before anything that can block.
    heartbeat.touch(clock.monotonic());

    let tick: Tick = match tokio::time::timeout(config.fetch_timeout, source.fetch(meter)).await {
        Ok(result) => result,
        // The deadline elapsed: the cloud is silent. That is a verdict input,
        // not an error to swallow.
        Err(_elapsed) => Err(SourceError::Timeout),
    };

    let (next, published) = policy.step(previous, &tick, clock.wall());

    if let Ok(reading) = tick {
        let update = MeterUpdate::new((*meter).clone(), reading.value, published);
        if outbox.send(update).await.is_err() {
            tracing::warn!(
                meter = %meter,
                "mqtt task is gone; dropping the judged reading"
            );
        }
    } else {
        // No reading to carry, but the verdict still matters: the mqtt task
        // republishes the last known value with this quality (Epic 2 wires the
        // republish; here the verdict is traced so a wedge is never silent).
        tracing::info!(meter = %meter, ?next, ?published, "no reading this tick");
    }
    next
}

/// The task: loops until the outbox closes.
pub async fn run<S: Source + Send>(
    meter: MeterId,
    mut source: S,
    clock: Arc<dyn Clock + Send + Sync>,
    policy: Policy,
    config: PollConfig,
    heartbeat: LastLoopTick,
    outbox: mpsc::Sender<MeterUpdate>,
) {
    let mut state = State::initial();
    let mut ticker = tokio::time::interval(config.interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if outbox.is_closed() {
            tracing::info!(meter = %meter, "outbox closed; poll task stopping");
            return;
        }
        let ctx = Context {
            meter: &meter,
            clock: clock.as_ref(),
            policy,
            config,
            heartbeat: &heartbeat,
            outbox: &outbox,
        };
        state = step_once(&ctx, &mut source, state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::clock::FakeClock;
    use crate::core::source::{FakeSource, Reading};
    use crate::domain::{Kw, Kwh, Measurement, Quality, Serial, UtcMillis};

    const SANE_NOW: i64 = 1_784_984_793_000;
    const BASE: i64 = 1_784_984_700_000;

    fn config() -> PollConfig {
        PollConfig {
            interval: Duration::from_secs(5),
            fetch_timeout: Duration::from_secs(2),
        }
    }

    fn policy() -> Policy {
        Policy { max_age_ms: 90_000 }
    }

    fn reading(quality: Quality, age_ms: i64) -> Reading {
        Reading {
            value: Measurement {
                meter: MeterId::new("garage"),
                serial: Serial::new("30000001"),
                power: Kw(0.018),
                energy: Kwh(4_843.822),
                value_date: UtcMillis(BASE),
                quality,
            },
            http_date: Some(UtcMillis(BASE + age_ms)),
        }
    }

    async fn drive(source: FakeSource) -> (State, Vec<MeterUpdate>) {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let heartbeat = LastLoopTick::new();
        let (tx, mut rx) = mpsc::channel(8);
        let mut source = source;
        let meter = MeterId::new("garage");
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let state = step_once(&ctx, &mut source, State::initial()).await;
        drop(tx);
        let mut got = Vec::new();
        while let Some(u) = rx.recv().await {
            got.push(u);
        }
        (state, got)
    }

    #[tokio::test]
    async fn a_fresh_reading_is_forwarded_as_good() {
        let (state, sent) = drive(FakeSource::new().then(Ok(reading(Quality::Good, 950)))).await;
        assert_eq!(state, State::Fresh);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].published, Quality::Good);
        assert_eq!(sent[0].meter, MeterId::new("garage"));
    }

    #[tokio::test]
    async fn an_out_of_bounds_age_is_forwarded_as_stale() {
        // The fetch succeeded; the timestamps say the data is old.
        let (state, sent) =
            drive(FakeSource::new().then(Ok(reading(Quality::Good, 600_000)))).await;
        assert_eq!(state, State::Stale);
        assert_eq!(sent[0].published, Quality::Stale);
        assert_eq!(
            sent[0].measurement.quality,
            Quality::Good,
            "the source's own view is preserved alongside the verdict"
        );
    }

    #[tokio::test]
    async fn a_transient_error_yields_stale_and_nothing_to_forward() {
        let (state, sent) = drive(FakeSource::new().then(Err(SourceError::Transient {
            reason: "503".to_string(),
        })))
        .await;
        assert_eq!(state, State::Stale);
        assert!(sent.is_empty(), "there is no reading to carry");
    }

    #[tokio::test]
    async fn a_fatal_error_latches_failed() {
        let (state, _) = drive(FakeSource::new().then(Err(SourceError::Fatal {
            reason: "auth rejected".to_string(),
        })))
        .await;
        assert_eq!(state, State::Failed);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_cloud_times_out_into_stale_instead_of_wedging() {
        // The localization twin of chaos_stale_on_cloud_timeout: the source
        // never answers, the REAL timeout path fires under paused time.
        let (state, sent) = drive(FakeSource::new().then_hang()).await;
        assert_eq!(state, State::Stale, "a silent cloud is STALE, not a hang");
        assert!(sent.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_heartbeat_is_written_before_the_network_call() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        // Advance BEFORE the step so "the instant at the top of the loop" and
        // "the instant after the fetch" are different numbers: without this the
        // assertion below would hold whichever side of the fetch the touch sat
        // on, and would prove nothing.
        clock.advance_ms(7_000);
        let heartbeat = LastLoopTick::new();
        assert_eq!(heartbeat.last(), None, "never run yet");
        let (tx, _rx) = mpsc::channel(8);
        // A source that never answers: if the heartbeat were written after the
        // fetch, it would still be None when the timeout fires.
        let mut source = FakeSource::new().then_hang();
        let meter = MeterId::new("garage");
        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let _ = step_once(&ctx, &mut source, State::initial()).await;
        assert_eq!(
            heartbeat.last(),
            Some(MonotonicMs(7_000)),
            "a hung fetch still leaves a heartbeat — that is what makes a wedge visible"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn recovery_needs_one_proven_reading() {
        let clock = FakeClock::new(UtcMillis(SANE_NOW));
        let heartbeat = LastLoopTick::new();
        let (tx, mut rx) = mpsc::channel(8);
        let mut source = FakeSource::new()
            .then(Err(SourceError::Timeout))
            .then(Ok(reading(Quality::Good, 950)));
        let meter = MeterId::new("garage");

        let ctx = Context {
            meter: &meter,
            clock: &clock,
            policy: policy(),
            config: config(),
            heartbeat: &heartbeat,
            outbox: &tx,
        };
        let after_timeout = step_once(&ctx, &mut source, State::initial()).await;
        assert_eq!(after_timeout, State::Stale);

        let after_good = step_once(&ctx, &mut source, after_timeout).await;
        assert_eq!(after_good, State::Fresh);
        drop(tx);
        let u = rx.recv().await.expect("the good reading was forwarded");
        assert_eq!(u.published, Quality::Good);
    }
}
