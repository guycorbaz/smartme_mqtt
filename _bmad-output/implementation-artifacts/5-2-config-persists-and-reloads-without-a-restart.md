# Story 5.2: Configuration persists across restarts and image updates, and takes effect without one

Status: review

> **Header corrected 2026-08-08.** This file said `ready-for-dev` while `sprint-status.yaml` said
> `review`. See the same correction on story 5.1.

> **Amended 2026-08-04 — [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md)
> supersedes ADR 0022.** `config.toml` is the whole configuration; the smart-me credential
> (`SMARTME_CLIENT_ID` + `SMARTME_CLIENT_SECRET`) stays in the environment and never descends to
> disk. **AC2 and AC3 are withdrawn** — there is no second file to protect and none to
> desynchronise. A **new AC2** takes their place, for the state the decision creates: a bridge with
> no configuration at all. **AC6 is retargeted** at the environment, where the secret now enters.
>
> **And the story is no longer blocked.** [#41] blocked it because a `0600` file sat in a
> world-writable directory; with nothing confidential in `/data`, that argument is spent. What
> remains is integrity, which is real and is nobody's prerequisite.

## Story

As the operator,
I want a setting I change to survive a restart and an image update, and to take effect without
either,
so that reconfiguring the bridge is not a deployment.

## Acceptance Criteria

The epic contributes **FR27** (*"persist configuration across restarts and image updates"*) and
**AR8** (*ArcSwap config*). Both stand. **Five are added**, for reasons in *Dev Notes*.

**AC1 — configuration survives a restart and an image update**

**Given** a configuration written to the state directory
**When** the container is restarted, and separately when the image is replaced with a newer tag
**Then** every setting is still in force
**And** the test exercises the **image-update** path, not only the restart path.

> These are two different claims and only one of them is cheap to test. A restart re-reads a file
> the same process wrote; an image update replaces the binary that reads it. The second is what
> FR40 promises the operator and what a schema change would break — see AC5.

**AC2 — no configuration is not an invalid configuration** *(replaces the withdrawn AC2/AC3, [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md) §5)*

**Given** a state directory with **no** `config.toml`
**When** the bridge starts
**Then** the process comes up and **stays** up, ready to serve the web UI
**And** it opens **no MQTT session**: no CONNECT, no will registered, no NBIRTH, no DBIRTH, nothing
published
**And** the reason is traced at a level visible under the **default** filter, saying the bridge is
unconfigured and where the file will be written.

**And, separately:** given a `config.toml` that **exists** and is invalid, the bridge refuses to
start, exactly as Story 5.1 specifies. **The two cases must be distinguishable in the code, not
merged** — collapsing them either bricks the first run or lets a corrupt file be treated as a fresh
install and silently overwritten.

> Everything but the credential arrives through a browser, so a bridge that refuses to start without
> a configuration can never be configured. **Absence is not invalidity.** The trace level is called
> out because this project has already shipped two acceptance criteria written in terms of a level
> that sat below the default filter, so nobody could see them.
>
> **The absence assertion here is the load-bearing one and it is falsifiable the wrong way:** "no
> NBIRTH appeared" holds trivially over a broker nothing ever connected to. Prove the harness sees a
> CONNECT when a configuration *is* present, then prove it sees none when it is not.

**AC4 — a change takes effect without a restart, and the meter set is a special case** *(added)*

**Given** AR8's `ArcSwap`
**When** a setting changes
**Then** the running bridge picks it up without restarting the process
**And** for the **meter set** specifically, enabling a meter publishes a **DBIRTH** and disabling one
publishes a **DDEATH**, under the same `bdSeq` — no new session, no NBIRTH, no interruption to the
other meters.

> A meter is a Sparkplug *device*, not a node metric, so this costs a device-level certificate rather
> than a rebirth. What the norm warns about
> (`Sparkplug_5_Operational_Behavior.adoc:863`) is the opposite: DDATA carrying a metric that was not
> in the previous DBIRTH — which publishing the DBIRTH **first** avoids. Order matters and is the
> testable part.
>
> **Not every setting is hot-swappable and the story must say which are not.** Changing the group or
> node identity changes the topic namespace and the will registered at CONNECT; that is a new
> session by definition. Decide per field at drafting: hot, or requires-reconnect. **No field may be
> left undecided** — an unlisted field will be assumed hot by whoever adds the form.

**The per-field table, decided 2026-08-04.** Four costs, named for what the *operator* sees rather
than for what the code does — "restarting the poll task" and "rebuilding the HTTP client" are the
same event to someone watching Ignition, namely nothing.

| setting | cost | why |
| --- | --- | --- |
| `publish_period_secs` | **hot** | the poll loop re-reads it each tick |
| `api_base` | **process restart** | *(was `hot`, corrected by review 2026-08-04 — [#52])* the client holding it is built once, before the poll task exists, and **nothing rebuilds it** |
| `meters[].device_id` | **process restart** | *(was `hot`, same correction)* it is moved into the source at construction |
| staleness policy, fetch timeout | **hot** | genuinely re-read from the handle on every tick |
| HTTP timeout | **process restart** | *(was `hot`, same correction)* consumed by the client at construction |
| `meters[].enabled` | **device certificate** | DBIRTH on enable, DDEATH on disable, same `bdSeq` |
| `meters[].serial` | **device certificate** | the serial IS the device topic level, so it is one device replaced by another: DDEATH then DBIRTH |
| `group_id`, `node_id` | **new session** | the topic namespace *and* the will registered in the CONNECT packet |
| `broker_host`, `broker_port` | **new session** | self-evident |
| state directory | **new session** | `bdSeq` is read at connect and written across restarts |
| `ui_port` | **process restart** | *(added by Story 6.1 as **new session**, corrected by review 2026-08-04)* moving a listener produces no NDEATH, no new `bdSeq`, no NBIRTH — the session is untouched. **The compiler asked for this row and could not check the answer**: it forced a classification and the classification was wrong |
| `log_dir`, `log_keep` | **process restart** | the tracing subscriber is installed before the configuration is read and cannot be re-pointed |
| smart-me credential | **process restart** | it is not in the file at all ([ADR 0023]) |

**The table is enforced by the compiler, not by this document.** `app::reconfigure::classify`
destructures `BridgeConfig` exhaustively, with no `..` rest pattern, so adding a field stops the
build until somebody says what changing it costs. A table alone would simply not mention the next
field anybody added — which is the failure this AC's own note predicts.

[ADR 0023]: ../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md

**AC5 — an older file must not be silently misread by a newer binary** *(added)*

**Given** an image update where the configuration schema has changed
**When** the new binary reads the old file
**Then** it either migrates it or refuses to start, and **never** starts with defaults substituted
for fields it did not understand
**And** the file carries a version so that "did not understand" is detectable at all.

> Serde's default behaviour is the trap: unknown fields are ignored and missing fields take their
> `Default`. A renamed field would therefore read as *absent*, take its default, and the bridge would
> start **happily on a configuration the operator never wrote** — publishing at 30 s because the
> period silently reverted. That is the same class as `bdSeq`'s corrupt-file fallback, which this
> project already handles by falling back to a sentinel **and logging it**.

**AC6 — no secret reaches the log, the error, or the state** *(added)*

**Given** a fault involving `SMARTME_CLIENT_SECRET` *(retargeted at the environment — the secret no
longer rests in a file)*
**When** it is reported
**Then** the message carries the **variable name**, never the value
**And** a test asserts the secret string appears in no log line, no error, and no published payload
**And** `config.toml`, having been written by this process, is asserted **not to contain it** — the
one place a refactor could put it back without any test noticing.

> ADR 0019's rule, tested here rather than in Epic 6, because the environment is where the value
> enters the process. A `Debug` derive on a struct carrying it would defeat this silently, and no
> template is involved — which is the whole argument for the Epic 5 / Epic 6 split. `RawConfig`'s
> hand-written `Debug` **stays**: the struct the secret arrives through is exactly where the leak of
> Story 1.6 happened.

## Tasks / Subtasks

- [x] **Task 1 — read before writing**
  - [x] `crates/smartme-bridge/src/persist.rs` in full — `persist_atomic` is TOML + temp + fsync +
        rename + fsync(dir), and it already exists. Do not write a second writer.
  - [x] `main.rs:233` — the state directory and its `/data` default.
  - [x] Story 5.1's fault collection: this story adds to it, it does not start a second one.
  - [x] [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md)
        in full. **Not ADR 0022** — it is superseded, and its `secrets.toml` no longer exists.

- [x] **Task 2 — one file, and the unconfigured state** (AC: 2)
  - [x] **Delete what ADR 0022 left behind**, committed at `6476412`: `StoredSecrets`,
        `secrets_path`, `check_mode`, `persist_atomic_with_mode`, and the three `store.rs` tests
        that exercise them. Deleting a test is a claim; say in the commit which AC each one served
        and why that AC is gone.
  - [x] **Keep** `RawConfig`'s hand-written `Debug` (AC6) — the secret still transits it.
  - [x] Wire `store::load` into `main.rs`, which today has no caller for it. `store::exists`
        is the seam: absent → serve the UI and stay off the wire; present → load, validate, refuse
        on fault.
  - [x] Withdraw the eleven environment variables from `main.rs`; keep `SMARTME_STATE_DIR`,
        `SMARTME_CLIENT_ID`, `SMARTME_CLIENT_SECRET`.
  - [x] **Logging is initialised before the configuration is read today, and `LOG_DIR`/`LOG_KEEP`
        are moving into the file.** That ordering has to invert. `main.rs` already writes faults to
        `stderr` as well as the log precisely for a start with no log destination — check that the
        no-configuration and invalid-configuration paths both still reach a human.

- [x] **Task 3 — versioned schema** (AC: 5)
  - [x] A version field, and `deny_unknown_fields` — the default of ignoring them is the defect.
  - [x] Decide migrate-or-refuse **now**, not when the first schema change happens.

- [~] **Task 4 — hot reload** (AC: 4) — *hot fields and device certificates DONE; a new-session
      change is classified and reported but NOT carried out. Recorded as unmet:
      [#49](https://github.com/guycorbaz/smartme_mqtt/issues/49).*
  - [x] `ArcSwap` — **a new dependency.** It reaches `Cargo.lock` and `deny.toml`; stage those files
        by name. Never `git add` a directory after a dependency change.
  - [x] The per-field table: above, and **enforced by an exhaustive destructure** rather than by
        this document — a table alone would not mention the next field anybody adds.
  - [x] DBIRTH on enable, DDEATH on disable, same `bdSeq`, DBIRTH before any DDATA. Proven against
        a real broker by `chaos_device_certificates.rs`, which counts NBIRTHs and requires exactly
        one. Conformant per `tck-id-message-flow-device-birth-publish-nbirth-wait`, read in the
        vendored spec rather than remembered.

- [x] **Task 5 — falsification** (AC: all) — *twelve mutations run 2026-08-04, all red, records
      copied next to their tests. The log-search for the secret is
      `secret_never_reaches_the_log.rs`, falsified by reproducing Story 1.6's defect exactly: a
      derived `Debug` on `Credential` plus the one trace line somebody plausibly adds beside it.
      **Neither half leaks alone**, which is why this is a test about the process rather than a rule
      about a derive.*

- [x] **Task 7 — AC1's image-update path** — `config_survives_an_image_update.rs`.
  - [x] **The file is written as TEXT, by hand, never through `store::save`.** A round-trip through
        the writer proves only that the writer and the reader agree with each other — which is
        exactly the assurance an image update removes. What distinguishes an update from a restart
        is that the code reading the file may not be the code that wrote it, so the *file* is what
        has to be tested, not a second toolchain run.
  - [x] Every stored value asserted, not a spot check: *"it loaded"* would also be true of a reader
        that substituted its own defaults for everything.
  - [x] Optional keys a previous build never wrote take their **documented defaults**, which is a
        different claim from *"they are absent"*.
  - [x] A file from another schema is **refused, naming the version it found**. Falsified by
        disabling the version check — and the telling part is that **the three other tests stayed
        green** under that mutation, because a file from another schema parses perfectly well. That
        is the whole danger.
> **These four boxes were unticked while three of them were done.** Verified 2026-08-08 by reading
> the tests rather than the record; each is ticked below with the artefact that satisfies it, so a
> future reader is not told work is owed that exists.

  - [x] Schema: rename a field in the file and confirm refusal rather than a silent default.
        `StoredConfig` is `#[serde(deny_unknown_fields)]` (`app/store.rs:98`), and the falsification
        is recorded at `app/store.rs:702` — removing it left the test green *because the unknown key
        was appended at the end of the file*, which in TOML puts it in the last table.
  - [x] Secrets: assert absence from logs by **searching for the value**, having first confirmed the
        search finds it when deliberately leaked — an absence assertion over a stream that never
        carried it proves nothing. `tests/secret_never_reaches_the_log.rs`: one matcher, `leaks()`,
        run first over the same text with a leak spliced in.
  - [x] **The unconfigured state (AC2), both directions.** First prove the harness *observes* a
        CONNECT when `config.toml` is present; only then assert none appears when it is absent.
        Reversed, the test passes against a bridge that never connects under any circumstances.
        The wire half is `tests/unconfirmed_publishes_nothing.rs::an_unconfigured_bridge_never_reaches_the_broker_either`,
        on a real broker, paired with a birth on the same harness. `unconfigured_start.rs` carries
        the process half and says in its own header that it never touches a broker — a claim that
        had been false there once and was corrected.
  - [ ] `./scripts/ci-local.sh`, full. Never piped — the exit code becomes `tail`'s. **Still owed
        for this story specifically**; it has been run since, for later stories, over the same tree.

- [x] **Task 6 — the manual, and the deployment** (AC: 2, 6)
  - [x] `docs/manual/chapters/04-configuration.tex` and `09-appendix-config-reference.tex` document
        the eleven withdrawn variables. The manual documents behaviour that **exists**, so it is
        amended in the same commit as the code, not before and not after. Its statement that secrets
        live in `.env` at `0600` was right all along and stays.
  - [x] The manual gains `config.toml`: its schema, its version field, and the fact that **writing it
        by hand is the supported headless bring-up** (rescoped FR23).
  - [x] `.env.example` loses the same eleven. Guy's deployment `.env` on panoramix does too — flag
        it rather than assume, since the file is not in the repository.

## Dev Notes

### The blocker this story inherited, and no longer does

~~**ADR 0022 is accepted; its prerequisite is not met.**~~ **Lifted 2026-08-04.** The blocker was
that a `0600` file sat inside a world-writable `/data` ([#41]) — a claim about one inode in a
directory where files can be replaced. [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md)
removes the file: nothing confidential rests in `/data` at all, so the confidentiality argument has
no subject.

**What is left is integrity, and it is not nothing:** whoever can write `/data` can replace
`config.toml` and point the bridge at another broker. It is a lesser risk, it is not this story's
prerequisite, and it is recorded in ADR 0023 under *What this ADR does not decide* rather than
dropped. [#41] stays open as a deployment task.

### What this story does NOT do

**No UI.** Per ADR 0021 the screens are Epic 6. Everything above is testable without HTML — the
write-only rule (AC6) especially, which is the reason the split exists.

~~**No multi-meter runtime.** Story 5.1's AC6 keeps the bridge refusing to serve more meters than it
can. AC4's DBIRTH/DDEATH applies to the meter the runtime does serve, and generalises when the
fan-out lands.~~

**Overtaken 2026-08-06 by story 3.1, recorded 2026-08-08, REPAIRED the same day.** The fan-out
landed: every enabled meter is served and the refusal is gone. **AC4's DBIRTH/DDEATH did not
generalise with it**, and the closing review's first reading of that gap was too kind — it called
the leftover verdict *"conservative rather than accurate"*. It was **wrong, and it withheld a
certificate**.

`classify_meters` inferred the served meter as *"the first enabled one in `old`, or the first one"*.
With four meters running, disabling the second, third or fourth was therefore classified
`ProcessRestart`: **no DDEATH was sent**, and the screen told the operator a restart would settle
it. A Sparkplug host went on showing a meter the operator had just switched off, at its last value,
as current. That is ADR 0027's withheld verdict — the failure this project is named for — reached
from the configuration screen instead of from a failed poll. Guy runs four meters, so it was
reachable on three of them.

**The repair is to stop inferring.** No configuration can answer "which meters does the runtime
serve": `old` says what is *desired* and is rewritten by every `apply`, so a meter enabled ten
minutes ago sits there as enabled while nothing polls it. The set now comes from
`Heartbeats::meters` — one entry per spawned poll task — and `classify` takes it as an argument.
That is also the seam story 3.1 named as missing (*"nothing asserts that `supervisor` spawns one
task per meter; the heartbeat count is the seam that would prove it"*); it is load-bearing now.

Three tests, in `app::reconfigure`: every position in a four-meter fleet buries its own device and
only its own; a meter that kept its task across a disable is **born** again rather than deferred to
a restart; and a meter the runtime never started still needs a restart however enabled the file
says it is — the guard that keeps the fix from over-reaching into declaring a device nothing polls.
Falsified by restoring the inference: `left: []` against `right: [Serial("9202686")]`, dying on the
second meter, with **eleven other tests still green** — every one of them describes a one-meter
bridge, which is how the defect survived the fan-out.

It also made `supervisor`'s own test harness admit something: it built `Heartbeats::for_meters(
["meter-a"])` while its configuration described `garage`. A served set unrelated to its own
config, invisible for as long as nothing read it.

### The absence assertion, again

AC5's secret test is an absence assertion, and this project has been caught by one: every chaos test
pointed at TEST-NET-1, so *"no DATA appeared"* held over an empty stream. **Prove the stream carries
something first**, then prove the secret is not in it.
