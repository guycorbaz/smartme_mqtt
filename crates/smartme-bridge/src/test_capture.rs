//! Capturing `tracing` output in a test, without letting a callsite be switched
//! off for the whole process ([#94]).
//!
//! # The defect this module exists to remove
//!
//! `tracing` caches each callsite's [`Interest`] **globally, for the process**,
//! while `tracing::subscriber::set_default` installs a subscriber **per thread**.
//! `tracing-core` decides a newly reached callsite's interest like this
//! (`callsite.rs`, 0.1.36):
//!
//! ```text
//! fn rebuilder(&self) -> Rebuilder<'_> {
//!     if self.has_just_one.load(SeqCst) {
//!         return Rebuilder::JustOne;   // <- asks dispatcher::get_default(),
//!     }                                //    i.e. THIS thread's subscriber
//!     Rebuilder::Read(LOCKED_DISPATCHERS.read().unwrap())
//! }
//! ```
//!
//! and `NoSubscriber::register_callsite` answers `Interest::never()`. So when at
//! most one dispatcher is registered — the ordinary case, since only a handful of
//! tests capture anything — a callsite first reached by a thread with **no**
//! subscriber in scope is cached as *never* for every thread, including the one
//! that is capturing. The capture then comes back empty, having missed events
//! that were produced correctly.
//!
//! That is the whole of [#94]: four occurrences on three tests, all inside full
//! workspace runs, never reproduced in 42 targeted attempts — because reproducing
//! it needs another thread to reach the callsite first, which no amount of CPU
//! pressure arranges.
//!
//! # The repair, and why it is one line of consequence
//!
//! [`capture`] keeps a **permanent global subscriber** installed, which answers
//! `Interest::sometimes()` to every callsite and nothing else. Two things follow,
//! and between them they close the hole whichever branch `rebuilder` takes:
//!
//! - with the global alive there is always at least one registered dispatcher, so
//!   a scoped one makes two and `rebuilder` reads the LIST rather than asking the
//!   current thread;
//! - and on the `JustOne` branch — a thread with no scoped subscriber — the
//!   answer is now this global's `sometimes` instead of `NoSubscriber`'s *never*.
//!
//! `sometimes` means "ask `enabled` per event", and the dispatcher asked is the
//! CURRENT one: the capturing thread's own subscriber decides, exactly as before.
//! This global never sees an event on a capturing thread and refuses every event
//! on any other, so it changes what is captured in no way at all.
//!
//! It is set with `set_global_default`, which can only be called once per
//! process; the result is deliberately ignored, because a process that already
//! has a global default has one that is at least as permissive as this.

use std::sync::{Arc, Mutex, Once};

use tracing::subscriber::Interest;
use tracing::{Event, Metadata, span};

/// Everything a subscriber wrote, shared with the test that reads it.
#[derive(Clone, Default)]
pub(crate) struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    /// The captured log as text.
    pub(crate) fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("not poisoned").clone()).expect("utf-8")
    }
}

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("not poisoned").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// A subscriber that keeps every callsite reachable and receives nothing.
///
/// `register_callsite` is the only method that matters: answering `sometimes`
/// means the decision is deferred to `enabled` on whichever dispatcher is current
/// when the event happens. `enabled` here says no, so a thread that is not
/// capturing logs nothing — which is what a test run should do.
struct KeepsCallsitesReachable;

impl tracing::Subscriber for KeepsCallsitesReachable {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        false
    }
    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }
    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
    fn event(&self, _: &Event<'_>) {}
    fn enter(&self, _: &span::Id) {}
    fn exit(&self, _: &span::Id) {}
}

/// Runs `body` with a capturing subscriber installed on this thread, and returns
/// everything it logged.
///
/// **Use this rather than `set_default` directly.** The permanent global it
/// installs on first use is what stops a callsite being cached as *never* by a
/// thread that is not capturing — see this module's documentation.
///
/// For a test that must hold the subscriber across an `await`, use
/// [`capture_guard`]: a closure cannot span one.
pub(crate) fn capture(body: impl FnOnce()) -> String {
    let (sink, guard) = capture_guard(tracing::Level::TRACE);
    body();
    drop(guard);
    sink.text()
}

/// Installs a capturing subscriber on this thread until the guard is dropped, and
/// hands back the sink to read afterwards.
///
/// The async half of [`capture`], and the reason both exist: `with_default` takes
/// a closure, and a closure cannot hold across `.await`. The protection is
/// identical — it is this function that installs the permanent global, and
/// [`capture`] goes through it.
pub(crate) fn capture_guard(
    level: tracing::Level,
) -> (Captured, tracing::subscriber::DefaultGuard) {
    static GLOBAL: Once = Once::new();
    GLOBAL.call_once(|| {
        // Ignored on purpose: see the module documentation.
        let _ = tracing::subscriber::set_global_default(KeepsCallsitesReachable);
    });

    let sink = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(sink.clone())
        .with_max_level(level)
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    (sink, guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A callsite first reached by a thread that is NOT capturing must still be
    /// capturable afterwards ([#94]).
    ///
    /// # The ORDER is the whole test, and the first draft had it wrong
    ///
    /// Registering the callsite from a bare thread and capturing afterwards does
    /// NOT reproduce it: building a `Dispatch` calls `register_dispatch`, which
    /// rebuilds the interest of every callsite already known — so a callsite
    /// cached as *never* before the capture exists is repaired by the capture's
    /// own construction. That draft passed with the repair removed, which is what
    /// exposed the mistake.
    ///
    /// The defect needs the other thread to arrive **while** the capture is
    /// live: the scoped dispatcher is then the only registered one, `has_just_one`
    /// is true, and a callsite reached for the first time off-thread is resolved
    /// against `NoSubscriber` and cached *never* — after which the capturing
    /// thread, which is asking for that very callsite, gets nothing.
    ///
    /// That is also why 42 attempts under CPU pressure reproduced nothing:
    /// pressure does not change who arrives first.
    ///
    /// **Run it alone to falsify** — `has_just_one` is only true while at most one
    /// dispatcher is registered, so a concurrent test holding a subscriber hides
    /// the defect, which is why the original flake was rare rather than constant.
    ///
    /// **FALSIFIED 2026-08-23**, `--exact` on this test alone, with the
    /// `set_global_default` call removed from `capture_guard`: RED, `the log was:
    /// ""`. Restored: green.
    /// The mutation is the code as it stood before this module existed.
    #[test]
    fn a_callsite_first_reached_without_a_subscriber_is_still_capturable() {
        // ONE function, so both threads go through ONE callsite. Two `warn!`
        // invocations on two lines are two callsites, and a draft that wrote them
        // out twice proved nothing — each thread got its own, and neither could
        // switch the other off.
        fn emit() {
            tracing::warn!(needle = "94", "the callsite under test");
        }

        let log = capture(|| {
            // A thread with NO subscriber in scope reaches the callsite FIRST,
            // while this thread is capturing.
            std::thread::spawn(emit)
                .join()
                .expect("the scratch thread does not panic");

            emit();
        });

        assert!(
            log.contains("needle=\"94\""),
            "a callsite another thread reached first must not be switched off for \
             this one: an empty capture makes a test red for a reason that has \
             nothing to do with what it measures, and that is what blocked a \
             pre-push gate twice. The log was: {log:?}"
        );
    }
}
