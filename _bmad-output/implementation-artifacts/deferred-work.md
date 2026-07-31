# Deferred Work

Items deferred from reviews; each carries its origin and where it should be picked up.

## Deferred from: code review of 1-2-domain-measurement-newtypes-quality (2026-07-25)

- String-key semantics before Epic 2 validation: `Serial::new("")` keys collide and `Eq`/`Hash` are case-sensitive (API vs config casing drift would create two logical devices) while Stories 1.9/1.10 already key maps on `Serial`/`MeterId`. Validation is scoped to Epic 2/5 per the story spec — revisit no later than Story 1.9.
- `TopicPath` accepts strings that are invalid as MQTT publish topics (empty string, `+`/`#` wildcards, interior NUL, leading/trailing `/`); failure surfaces only at the broker, far from the construction site. Well-formedness lands inside `new()` in Epic 2/5.
- Range policy unstated for physical/timestamp values: negative or `±inf` `Kwh` (a cumulative counter), pre-1970 `UtcMillis`, and the eventual `i64→u64` conversion at the Sparkplug encode boundary (wire timestamps are `uint64`). Epic 2 oracles / Story 1.8 own these guards.

## Deferred from: code review of 1-5-pure-fresh-stale-failed-state-machine (2026-07-25)

- Frozen/replayed-feed oracle: a byte-identical replayed response (`http_date` frozen together
  with `value_date`) keeps a plausible age and stays Fresh. Detection needs cross-tick state
  (`http_date` strictly advancing between accepted readings) — exactly the "per-serial state
  already carries the inputs" additive oracle the architecture defers to Epic 2. Same state also
  catches the future-dated coherent pair (both stamps shifted, small age).
- `Date`-header 1-second truncation tolerance: a genuinely fresh reading can compute age ≈ −900 ms
  and go spuriously STALE (flapping at sub-second phase alignment). Spec-literal `age < 0 → STALE`
  kept for Epic 1 (fail-safe: noise, never a lie); revisit the tolerance band (e.g. −2000 ms) with
  real polling data in Epic 2.
- `Policy::max_age_ms` validation (reject ≤ 0 at config load) — Epic 3 config oracle.

## Deferred from: code review of 1-6-smart-me-client (2026-07-25)

- One-shot token-refresh-then-retry on 401 after a previously-valid token (ADR 0009) — the
  bridge adapter owns that loop; land it in Story 1.7/1.11.
- 429 `Retry-After` honoring + rate-limit backoff — Epic 2 (architecture: "rate-limit/429
  backoff + token-refresh handling").
- Tolerant per-device list deserialization (today one degraded device fails the whole
  `GET /Devices` array parse; single-device path unaffected) — Epic 2 robustness.

## Deferred from: code review of 1-7-smartme-cloud-source (2026-07-25)

- Token-lifecycle tests (expiry boundary, 401 refresh-then-retry, double-401 → Fatal): need an
  injectable client seam or an HTTP stub the workspace deliberately lacks. Land alongside the
  Story 1.11 task tests.
- `map_device` diagnostics: the three failure modes (unknown unit / non-finite value /
  unparseable ValueDate) collapse into one undifferentiated `Bad` with no record of which field
  or which raw string failed. Epic 2 diagnostics/culprit classification owns this.
- `Bad` carrier value `0.0` on a cumulative kWh counter: a consumer that ignores the quality flag
  sees the counter crash to zero and snap back (a huge negative then positive delta). Revisit
  against real Ignition behaviour in Story 1.15 / Epic 2 — options: last-known-value carrier, or
  omit the metric entirely.

## Deferred from: code review of 1-8-sparkplug-b (2026-07-25)

- `Node Control/Rebirth` metric in NBIRTH + the NCMD decode path that acts on it, and host
  STATE handling. A Sparkplug-conformant edge node needs both; declaring the Rebirth capability
  without being able to act on it would be its own lie, so the crate documents the gap in its
  Conformance scope instead of stubbing it. Epic 3 owns the command path.
- Device-level (D*) messages, metric aliases, templates/datasets, topic-string construction:
  out of the walking skeleton's scope; the caller owns its topic namespace today.
- BIRTH-declares / DATA-validates metric registry (a consumer may discard DATA for a metric the
  BIRTH never declared), plus guards for an empty metric name and an empty BIRTH.
- `is_historical` on replayed buffered data — only relevant once a broker-outage buffer exists
  (v1 policy is traced-drop, no buffer).

## Deferred from: code review of 1-9/1-10 (2026-07-25)

- `arch_purity`'s mapping-confinement guard is a text proxy: it trips only on a file containing
  BOTH `Measurement` and `sparkplug_b::`. A future task file taking `&MeterUpdate` (which does
  not contain the token `Measurement`) could duplicate the mapping undetected, and the scan
  covers only `src/adapters/`. Strengthen when Story 1.12 lands its file.
- Story 1.10's second AC ("both tasks reference this type, neither redefines it") cannot close
  until Stories 1.11/1.12 exist; it needs its own purity clause then.
- Report-by-exception / duplicate suppression: an unchanged `value_date` republishes the same
  point every poll (duplicate historian points at one timestamp). Epic 2.
- No plausibility floor on the publisher's `now`: an unsynced RTC at boot stamps the BIRTH
  certificate with 1970 (values stay Stale, so no value lies — but the certificate does).
- `Sink::emit` has no failure channel; an unpublishable message is indistinguishable from a
  delivered one at that layer (Story 1.12's broker-ACK requirement will need one).

## Deferred from: code review of 1-11/1-12/1-13 (2026-07-25)

- `poll_publish::run` (the loop) is untested; only `step_once` is. Needs a `start_paused` test
  covering ticker pacing, `MissedTickBehavior::Delay`, the `outbox.is_closed()` exit and state
  carry-over.
- bdSeq is persisted once at boot, not per session; a corrupt/missing file restarts at 1, which
  replays numbers a long-lived consumer has seen. Epic 3 config validation should refuse to start
  instead.
- Reconnect backoff is a fixed 1 s with no exponential growth or jitter: a broker down for an
  hour gets 3600 synchronized attempts.
- No NCMD subscription, so `Node Control/Rebirth` cannot be honoured (pairs with the 1.8 deferral).
- 1.11's channel test never asserts the `Measurement` payload (power/energy/value_date), only the
  meter id and the qualities.
- `arch_purity`'s `in_test_module` latch never resets: sound today (every `#[cfg(test)]` is the
  final item in its file) but silently blind if anyone adds a mid-file test helper.

## Deferred from: closing 1.13's `chaos_sigterm_no_lie` (2026-07-26)

- No chaos test cuts off a live stream of GOOD readings. Every chaos scenario points the bridge
  at an unroutable smart-me API, so the poll task never obtains a reading and no DDATA is ever
  produced — the "no fresh DDATA survives the death" clause is proven only in its general form
  (nothing at all reaches a subscriber after the certificate). Proving the narrow claim needs a
  TLS-terminating fake of the smart-me API, because `SmartMeClient` mandates HTTPS and the
  webpki roots reject a self-signed cert. Worth building once: it would also unlock testing the
  Good → Stale transition end to end, and rebirth-with-real-values.
- `chaos_sigterm_no_lie` shells out to `kill(1)` to send the signal, so it needs `kill` on PATH.
  A `libc` dev-dependency would be more robust; it was avoided because it is a dependency
  addition the story did not authorise, and `kill` is present on every target platform.
- **`chaos_stale_on_death` has the tautological `bdSeq` pairing that `chaos_sigterm_no_lie` was
  fixed for**: its state dir is created fresh, so `load_bd_seq` returns the sentinel and both
  sides of `death.bd_seq() == birth_bd_seq` are the same low constant on every run. Seed the
  persisted number there too.
- `mqtt_driver.rs`'s death-flush is `timeout(death_flush, sleep(death_flush))` — a tautology whose
  `flushed.is_err()` branch is a coin flip, so the "death flush timed out; falling back to the
  will" warning is meaningless and untested. Nothing verifies the certificate actually reached the
  wire before `pump.abort()`; on a loaded runner (jobs capped at 2, plus a container broker) the
  pump may not drain in time, which surfaces as a `chaos_sigterm_no_lie` failure indistinguishable
  from the real regression.
- The suite now treats a will-only graceful death as a hard failure, while `epics.md:696` still
  states the AC as a disjunction that permits it. Spec and gate disagree — the epic text needs
  amending (see the Story 1.13 notes).
- `shutdown_signal()` registers its SIGTERM handler only when first polled, at `supervisor.rs:129`.
  Everything before that — runtime build, client construction, both spawns — runs under SIGTERM's
  default disposition, i.e. immediate termination with no explicit death. A container runtime that
  stops the bridge during startup is therefore uncovered, and no test can reach that window.

## Deferred from: code review of 4-2-conformance-matrix-payloads-metrics-datatypes (2026-07-28)

- **Chapter 4 of `docs/sparkplug-conformance.md` is marked `done` but fails the completeness
  standard Story 4.2 invents.** Applying 4.2's own two mechanical checks to
  `docs/spec/sparkplug-b-3.0.0/chapters/Sparkplug_4_Topics.adoc`: it holds **70** `tck-id`s, of
  **CLOSED 2026-07-28 — now owned by Story 4.19**, and the count was wrong here: an independent
  recount gives **29**, not 27. Left in place as the record of how it was found. Original entry:
  which **27 have no row and appear in no collective block** — including `topics-nbirth-metrics`,
  `-nbirth-seq-num`, `-nbirth-timestamp`, `topics-ndeath-payload`, `-ndeath-seq`,
  `topics-ddata-seq-num`, and three `host-topic-phid-death-payload-timestamp-*` ids the STATE block
  omits. The stated tally "17 conformant · 0 deviations · 8 gaps · 21 n/a" (46) does not match the
  rows, which count **14 · 0 · 8 · 19** (41) — over-stated by 3 conformant and 2 n/a.
  Most pointedly, **`tck-id-topics-nbirth-bdseq-increment` is unrecorded**: that is chapter 4's own
  id for the per-CONNECT `bdSeq` increment, i.e. the exact Story-4.10 deviation the chapter-6 pass
  presents as a discovery. Pre-existing to Story 4.1 and not introduced by 4.2 — but 4.2 reaffirms
  `chapter 4 | 4.1 | **done**` in the Status table and adds an `epics.md` acceptance criterion
  ("every clause accounted for … the arithmetic … is stated") that chapter 4 does not meet.
  Decided 2026-07-28: **Story 4.19** owns the chapter-4 completion, rather than re-opening 4.1 whose
  work is correct as far as it goes and already pushed. No GitHub issue — CLAUDE.md requires an
  owning story *or* an issue, and the story is the stronger record.

## Deferred from: code review of 4-3-conformance-matrix-lifecycle-and-host-interaction (2026-07-28)

- **Chapter 6's `payloads-nbirth-bdseq-repeat` may be a wrong verdict against its own clause text.**
  It reads *"The bdSeq number value MUST match the bdSeq number value that was sent in the prior MQTT
  CONNECT packet WILL Message"* (`Sparkplug_6_Payloads.adoc:1075-1077`) — a requirement the bridge
  **satisfies**. Chapter 5's `-nbirth-payload-bdSeq` states the same testable requirement
  (`Sparkplug_5:224-226`) and this pass ruled it `conformant`. The increment obligation that makes
  the chapter-6 row a `deviation` appears in chapter 6 only as a **non-normative sub-bullet**
  (`:1521-1522`), whereas chapter 5 gives it its own id (`-will-message-payload-bdSeq`, ruled
  `deviation` here). So chapter 6 may be carrying a defect on a row whose clause is met, and a reader
  auditing that row against the norm will not find the defect it names. Not re-decided: chapter-6
  rows are outside Story 4.3's scope boundary. **Owner: Story 4.19**, alongside `topics-dcmd-topic`.
  Found by the Edge Case Hunter layer, which read both clause texts verbatim.

- **Chapter 4's `topics-dcmd-topic` is a `gap` where chapter 5's `-device-dcmd-subscribe` is `n/a`.**
  Under the matrix's own hold-the-datum criterion — no device declares a writable output, so no DCMD
  could address anything — both should be `n/a`. Recorded rather than changed, for the same scope
  reason. **Owner: Story 4.19.**

- **The "SIGKILLed" misdescription of `chaos_stale_on_death` originates in chapter 6.** The test
  aborts a tokio task in-process (`chaos_stale_on_death.rs:68`); there is no signal and no separate
  process. `chaos_sigterm_no_lie` is the real-process, real-signal test. Story 4.3 propagated the
  error into chapter 5 and has now fixed it **there only** — chapter 6's row at the NDEATH table
  still says it. **Owner: Story 4.19.** The witness itself is genuine (the socket drops, the broker
  fires the will, an independent subscriber receives it); only the mechanism was misstated.

- **No falsification has ever been aimed at the `n/a` column.** All nine mutations run for Story 4.3
  target `conformant` or `gap (unproven)` rows. `n/a` is **64 of 124** clauses in this pass and 144
  of 274 across the audited specification — the largest column, and the one the matrix itself calls
  the dustbin risk. It is checked only by re-reading, which is the method the same document dismisses
  ("an audit agreeing with itself proves nothing"). No mechanical falsification exists for "this
  clause binds a role we do not play", which is why it has not been done; inventing one — e.g. a
  script asserting every `n/a` clause's text contains a Host-Application or MQTT-Server subject — is
  a genuine open problem worth an epic-level decision. **No owner yet; raise at the Epic 4
  retrospective.**

- ~~**Eleven untracked dotfiles sit at the repository root**~~ — **WITHDRAWN 2026-07-29. The finding
  was false and there is nothing to do.** No such files exist. `ls -a` at the repository root shows
  only `.cargo`, `.claude`, `.env`, `.env.example`, `.git`, `.github`, `.gitignore`, and
  `git status` reports a clean working tree.

  **What was actually being observed.** The agent's command sandbox hides certain paths by mounting
  `/dev/null` over them. Inside the sandbox those paths therefore *appear to exist* — `ls -l` shows
  `crw-rw-rw- … 1, 3`, a character device, and `git status` lists them as untracked. `cat` returns
  "permission denied", which reads like a permissions quirk rather than the clue it is. Every tool
  run inside the sandbox agrees with every other, so the observation is internally consistent and
  entirely an artefact of the lens.

  **How it was caught.** Not by looking harder inside the sandbox — three passes there would have
  reported the same eleven files. It was caught by running the *same* `ls` with the sandbox disabled
  and getting a different answer. The device numbers (`1, 3` = `/dev/null`) then explained why.

  **This is the same failure shape as Story 4.4's retained-STATE snapshot**, and it is worth
  noticing that the project produced two instances of it in one week: a measurement that is careful,
  repeatable and consistent, taken through an instrument that silently alters what it reports.
  Repeating a measurement with the *same* instrument cannot detect it. See
  `docs/primary-host-state-observation.md` and `CLAUDE.md`'s falsification rule — the discipline
  generalises beyond tests.

## Deferred from: code review of 4-4-primary-host-state-measure (2026-07-29)

- **The `epics.md` ADR-number note is a third instance of the pattern it documents.** It writes a
  bold *"Next free is 0016 as of 2026-07-28"* directly beneath its own instruction to *"treat any ADR
  number written in this file as stale on sight and check `docs/adr/` instead"*. If the remedy is
  right the digit should go; if the digit is useful the remedy is overstated. Task 6 re-verified the
  number against `docs/adr/` and it is currently correct, so this is presentational rather than
  wrong. **Deferred; settle it the next time an ADR is numbered.**

- **The new manual chapter has never been reviewed by anyone.**
  `docs/manual/chapters/02-understanding-sparkplug.tex` — 732 lines, four TikZ figures, the
  `git mv` renumbering of eight chapters (02→03 … 09→10) and the shared-style additions to
  `preamble/style.tex` — was pushed in `150a57f` alongside Story 4.4 and was excluded from that
  story's review by an explicit scope choice, to keep the adversarial pass on the conformance
  argument. It builds (`latexmk` exit 0, 38 pages) and was verified page by page by its author, but
  no independent layer has read it. Its § 3.11 *Where smartme_mqtt sits* ranks every mechanism
  implemented / absent / deviation, which makes it exactly the kind of claim this project reviews.
  **Deferred; owner unassigned.**

## Deferred from: code review of 4-6-ncmd-subscription-plumbing (2026-07-29)

- **A SubAck that never arrives is indistinguishable from a granted subscription.**
  `mqtt_driver.rs:342-377` handles refusal, downgrade and an empty answer, but there is no deadline,
  no pending-subscribe state and no absence check. The comment claims *"the operator learns which it
  is from the log alone — no broker access needed"*; on this path the log is silent, and below INFO
  a healthy grant is silent too. **Deferred:** it needs its own mechanism and it interacts with the
  deliberate decision (taken at drafting time) not to block on the SubAck before birthing.

- **`Transport::Subscribed` is delivered by a blocking `send().await` on `pump_transport`.**
  Four lines below, the inbound-command arm carries the rule this task must obey — *"`try_send`,
  never `send().await`. THIS task is what answers PINGREQ"* (`mqtt_driver.rs:511-515`). The rule is a
  property of the task, not of the channel, so it applies to the 8-slot `transport_tx` too.
  **Deferred:** pre-existing shape shared with `Connected` and `Lost`; this story adds one more
  instance rather than the hazard. It becomes live the day an arm of the main `select!` awaits
  anything longer than the keep-alive — which is exactly what Story 4.5's *wait for the Primary Host
  before birthing* is.

- **The chaos test's fixed two-second settle guards a diagnostic property, on one validation.**
  `chaos_ncmd_subscription.rs` sleeps two seconds before reading the broker log once, so that a late
  SUBSCRIBE is reported as *late* rather than *absent*. The file already owns the right tool —
  `wait_for_log` polls until a needle appears — and uses it everywhere else. Under a loaded CI
  (`jobs=2` plus testcontainers) the constant could expire and send the reader to the harness instead
  of the ordering bug. **No false green is possible** (an unflushed log panics), so this is
  diagnostic quality, not correctness. **Deferred; settle it the next time the chaos suite is
  touched.**

- **An inbound publish whose topic does not match `ncmd_topic` is discarded with no trace.**
  `mqtt_driver.rs:508-510` falls through to `Ok(_) => {}` at `:521`, in a driver where every other
  drop is traced and the phrase *"never silently"* appears three times in the new code. **Deferred:**
  unreachable until a second subscription exists — which is Story 4.5.

- **A failed `try_subscribe` is never retried for the life of the session.**
  `mqtt_driver.rs:554-566` traces at ERROR and returns; the caller births regardless, which is the
  behaviour AC2 asked for. What is unhandled is the residual: nothing re-attempts on any later
  transport event, so the session runs unsubscribed until the next reconnect. **Deferred:** pairs
  with the packet-size decision item on the same story, and both are cheap to settle together.

- **An inbound MQTT packet above 10 KiB tears down the session and fires the will.**
  `rumqttc` defaults `max_incoming_packet_size` to 10 KiB and rejects the frame in
  `mqttbytes/mod.rs:181-183`, *before* `mqtt_driver.rs:508`'s topic guard is evaluated; the socket
  drops ungracefully, the broker publishes the will, and the host is told the node died while it was
  alive. Before Story 4.6 the bridge held no subscription, so no PUBLISH could reach its socket at
  all — **the subscription creates the path**. **Deferred by decision (Guy, 2026-07-29): the vector
  is strictly weaker than a capability the unauthenticated broker already offers, and the correct
  control is broker-side.** (a) It is a *disruption*, not a lie: the session genuinely dies, the
  death certificate is correct, and the bridge re-births ~1 s later. (b) No legitimate NCMD for this
  bridge approaches 10 KiB, so the default is correctly sized; raising it moves the cliff at the cost
  of the bounded memory AC-LEAK-01 protects. (c) The bridge cannot drop the packet and keep the
  session — that is rumqttc's deliberate behaviour. (d) **Any client on this broker can already
  publish a forged NDEATH on `spBv1.0/<group>/NDEATH/<node>`**, which lies immediately and needs no
  provocation. Mitigations belong to deployment: Mosquitto's `message_size_limit` (one line, and it
  protects Ignition too) and broker ACLs — Epics 5/7. **Unmeasured residue:** a sustained attack
  would churn death/birth at roughly 1 Hz on the host; hand that observation to **Story 4.13**
  (chaos broker recovery) rather than reasoning about it further.

## Deferred from: code review of 4-7-node-control-rebirth-answer (2026-07-31)

- ~~**LaTeX table overflow in the manual's NCMD-behaviour table.**~~ **CLOSED in the same review, by
  building it.** The item was written as a defer on the grounds that an overfull hbox is a *warning*,
  so the story's *"`latexmk` exits 0"* claim could not rule it out and confirming it needed a build.
  The build was then run: chapter 5's table was **`Overfull \hbox (17.75223pt too wide)`**, exactly as
  suspected, under an exit code of 0. Column one was `l`, which does not wrap, and Story 4.7's
  `\code{Node Control/Rebirth}, any other value` did not fit. Fixed by making both columns `p{}`
  (`p{4.3cm} p{8.3cm}`); the rebuild has no overfull box for that table, and the three that remain
  pre-date this work. **Recorded rather than deleted, for the lesson: `latexmk` exiting 0 says nothing
  about whether the page is right, and this repository already has a rule about exit codes describing
  something other than what was measured.**
