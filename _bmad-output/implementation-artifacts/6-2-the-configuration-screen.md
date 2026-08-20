# Story 6.2: The configuration screen — and the click that ends the silence

Status: review

## Story

As the operator,
I want to configure the bridge and confirm its mapping in a browser,
so that bringing it up does not require me to write a TOML file by hand.

## What this closes

Everything Epic 5 built has no caller. `Control::apply`, `Plan::cost()`,
`Plan::needs_restart()`, `config::mapping_preview` and `store::confirm` are written, tested,
falsified — and unreachable. This is the story where a browser reaches them, and where the product
stops requiring a text editor to start.

## Acceptance Criteria

**AC1 — a first run can be completed entirely in the browser**

**Given** a bridge with no `config.toml`
**When** the operator fills the form and saves
**Then** the configuration is written, validated by the **same** `app::config::validate` the file
path uses, and the bridge moves to *unconfirmed*
**And** a value the browser accepted is a value the bridge will boot on.

> One validation, not two. That is Story 5.1's whole reason for existing: *"two consumers assembling
> the same struct by hand is how their rules drift apart"*. The form must not pre-validate with its
> own copy of a bound — it may only render the faults `validate` returns.

**AC2 — every fault is shown at once, against the field it belongs to**

**Given** a submission with several things wrong
**When** it is refused
**Then** all of them are shown together, each beside its field
**And** the message is the one `Fault` already carries, including where the setting lives.

> `ConfigErrors` is already a collection for exactly this reason — the `?`-on-first-error shape made
> a first run up to six edit-restart cycles. A form that showed one fault at a time would rebuild
> that by hand. `Fault::source` already names `config.toml: group_id` or
> `environment: SMARTME_CLIENT_SECRET`; the second must not be rendered as an editable field.

**AC3 — confirming is a separate, deliberate act**

**Given** a saved configuration
**When** the operator reviews the mapping
**Then** the screen shows, per meter, **the serial beside the exact topic** and the device id
**And** confirming is its own submission, not a checkbox on the save form.

> `prd.md:136` calls this *"the only guard against a mis-map the machine cannot detect"*, and
> `mapping_preview` already returns exactly these fields. **A confirmation folded into the save
> button is not a confirmation** — it is a click the operator makes for a different reason, which is
> how a guard becomes a formality. Story 5.3's model already refuses to let a save assert it.

**AC4 — the screen says what a change will COST before it is made**

**Given** [Story 5.2's `Plan`](5-2-config-persists-and-reloads-without-a-restart.md)
**When** a change is saved
**Then** the screen reports its cost in the operator's terms — nothing, one device re-announced, a
new Sparkplug session, or *takes effect at the next restart*
**And** a field that was **not** applied is never reported as saved-and-in-force.

> `Plan::cost()` and `Plan::needs_restart()` exist for this and have no caller. **The
> new-session-class case is currently not carried out at all** ([#49]) — so the screen must say
> *"saved; takes effect when you restart"* and not *"saved"*. `Control::current()` deliberately
> keeps reporting the old value; the screen must render that rather than the value it just posted,
> or it will show the operator a change that has not happened.

**AC5 — a form cannot be submitted from somewhere else**

**Given** [ADR 0019](../../docs/adr/0019-no-auth-on-the-config-ui-secrets-are-write-only.md): no
authentication, LAN-only, Traefik the sole ingress
**When** a request arrives that did not come from this UI
**Then** it is refused.

> **This has to be decided here, and it is not covered by "no auth".** Precisely *because* there is
> no login, any page in the operator's browser can POST to the bridge if that browser can reach it —
> a drive-by reconfiguration needs no credentials to steal, because there are none. The blast radius
> is a Sparkplug namespace a host persists.
>
> **Decide one of:** (a) require a same-origin `Origin`/`Sec-Fetch-Site` header on every mutating
> request; (b) a per-render token in a hidden field; (c) nothing, and record why.
>
> **Recommendation: (a).** It costs one header check, has no state, cannot desynchronise, and works
> without cookies — which matters because there is no session to hang a token on. (b) would need
> somewhere to remember the token, which is the session ADR 0019 deliberately does not have.
> **Whatever is chosen must be written down**, because a future reader will find a no-auth UI and
> reasonably assume nothing was considered.

**AC6 — the screens never render a secret, because there is none to render**

**Given** [ADR 0023](../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md)
**When** the configuration form is rendered
**Then** there is **no credential field at all** — not empty, not masked, absent
**And** a test asserts the secret's value appears in no response body.

> ADR 0019's write-only rule lost its subject when the credential left the file: the strongest form
> of *never rendered* turned out to be *never present*. Epic 6's own note says it —
> *"there is no secret field"*. The test exists anyway, with the guard pattern
> `secret_never_reaches_the_log.rs` already uses: prove the search finds the value when it IS there,
> then prove it is not.

**AC7 — confirming must not require the operator to restart the container** *(added while drafting
the implementation — see below)*

**Given** a bridge sitting in *unconfigured* or *unconfirmed*, which is `run_without_publishing`
**When** the operator saves a valid configuration and confirms the mapping
**Then** the bridge begins publishing **without human intervention**
**And** if that is not delivered, the screen says plainly that a restart is needed — it never
implies publishing has begun.

> **This gap was found by writing the story's implementation notes, not by running anything, and it
> would otherwise have surfaced halfway through the work.**
>
> `run_without_publishing` waits for a shutdown signal and can do nothing else. It has no session to
> start, and `main.rs` decides the state once, before it. So as the code stands today, an operator
> who configures and confirms in the browser is met by a bridge that keeps saying *"nothing is
> published"* until somebody restarts the container — which turns the story's own promise, *a first
> run completed entirely in the browser*, into a first run completed in the browser and a terminal.
>
> **Decided: `main.rs` becomes a loop over the lifecycle.** `run_without_publishing` returns a
> reason — *shutdown* or *the configuration became ready* — the UI signals the second after a
> successful confirmation, and the loop re-reads the file and enters `run`. It is a small change and
> it is the honest one: the alternative, exiting so a restart policy picks it up, depends on a
> deployment setting the bridge cannot see and would look like a crash in the logs.
>
> **The transition is one-way in this story.** Going back — a confirmation withdrawn while
> publishing — means tearing the session down, which is [#49]'s problem and not this one. Withdrawal
> already costs a restart today and the screen must say so.

## Tasks / Subtasks

- [x] **Task 1 — read before writing**
  - [x] `app::supervisor::Control` — the whole API, and why `current()` reports what is IN FORCE.
  - [x] `app::reconfigure::Plan` — `cost()`, `needs_restart()`, and the four costs.
  - [x] `app::config::{validate, Fault, Source, mapping_preview}`.
  - [x] `app::store::{save, confirm}` — and **why `confirm` bypasses `save`**.
  - [x] `ui/mod.rs` — `Lifecycle` is the single source of truth; do not add a second.

- [x] **Task 2 — the form** (AC: 1, 2, 6)
  - [x] Server-rendered, no SPA (`architecture.md:217`). `axum`'s `Form` extractor.
  - [x] Faults rendered from `ConfigErrors`, never re-derived.
  - [x] No credential field. Assert its absence from the response body.

- [x] **Task 3 — confirmation** (AC: 3)
  - [x] Its own route and its own submission.
  - [x] The table shows serial, device id and the exact topic, from `mapping_preview`.

- [x] **Task 4 — cost reporting** (AC: 4)
  - [x] `Plan::cost()` rendered in the operator's terms.
  - [x] **Re-read `Control::current()` after applying** and render THAT, not the submission.

- [x] **Task 5 — same-origin** (AC: 5)
  - [x] The chosen mechanism, applied to every mutating route, with the decision written down.
  - [x] A test that a cross-origin POST is refused **and** that a same-origin one is not — the
        second half matters, or a guard that refuses everything would pass.

- [x] **Task 6 — falsification** (AC: all)
  - [x] Accept a period outside ADR 0020's bounds through the form and confirm the test catches it
        — that is the "the form validates too" defect.
  - [x] Report a new-session change as in force and confirm the test catches it.
  - [x] Fold confirmation into the save and confirm the test catches it.
  - [x] **Assert every mutation actually applied before running it.** On 2026-08-04 a mutation
        matched nothing because `rustfmt` had reflowed the target, and the test stayed green.
  - [x] `./scripts/ci-local.sh`, **full**.

- [x] **Task 7 — the lifecycle loop** (AC: 7)
  - [x] `run_without_publishing` returns a reason rather than `()`.
  - [x] `main.rs` loops: re-read the file, re-decide the state, enter the right runner.
  - [x] **The re-read is a full `store::read` + `validate`**, not a patch of what was posted — the
        file is the configuration, and a loop that trusted its own memory would be a second source.
  - [x] Falsify by signalling the transition without writing the file, and confirming the bridge
        does not publish.

- [x] **Task 8 — the consequences**
  - [x] `docker-smoke.sh`: a first run can be configured over HTTP end to end.
  - [x] The manual's chapter 6 is a stub; it gains the screens.
  - [x] NFR11 — *time-to-first-value under 15 minutes from a clean machine* — becomes measurable for
        the first time. **Measure it and record the number**, rather than asserting it.

## Dev Notes

### What this story does NOT do

**No live values, no diagnostics, no state screen.** FR28/29/30/36 are later stories. This one ends
at: a bridge can be brought from empty to publishing without a text editor.

### The one thing that will be tempting and is wrong

Validating in the form. It will feel helpful, it will give faster feedback, and it will be a second
copy of every bound in `app::config` — which is the drift Story 5.1 exists to prevent, and which
shows up as a value the browser accepts and the bridge refuses to boot on. Render `validate`'s
faults; add nothing.

### Where the traps have been

Five times on 2026-08-04 a change to how the binary starts made a check go **quiet rather than
red**. This story changes what the binary serves rather than how it starts, which is safer — but
`docker-smoke.sh` is still the file that catches it, and `--fast` still does not build the image.

---

## Implementation record — 2026-08-05

**All seven acceptance criteria met.** Fourteen mutations, all red; falsification records live
beside the tests (`tests/a_first_run_is_completed_in_the_browser.rs`, `app/phase.rs`).

### What was decided while building

- **AC5: a same-origin header check**, per the story's own recommendation. Recorded as
  **ADR 0024** and **[#54]**, because the story required it to be written down. Two details
  the story could not have known: the *scheme* must not be compared (Traefik terminates TLS
  in front of a plain-HTTP bridge, so `Origin` says `https` and the listener sees `http`),
  and a request with **no** `Origin` must be allowed — that is `curl`, which is how FR23's
  headless bring-up and `docker-smoke.sh` drive this surface.
- **The web server is started once and outlives every phase**, rather than being rebuilt per
  phase. Rebuilding would close and re-bind the listener, and a failed bind degrades to
  "no UI" at exactly the moment the operator has just used it.
- **A configuration that becomes invalid mid-session does not kill the process.** The startup
  rule (6.1 AC1) is untouched: a file present and invalid *at startup* still refuses to
  start. Later turns of the loop only happen because an operator is in the browser right now,
  and killing the process would destroy the repair tool over a file the bridge itself had
  just been asked about. This declines to extend a rule to a situation it was not written
  for; it does not weaken it.

### NFR11, measured rather than asserted

**1 second** from a clean state directory to a publishing bridge, container only — excluding
the image pull and the operator's typing. `docker-smoke.sh` prints the number on every run, so
a creep from seconds to minutes is visible rather than merely under a 900 s budget.

### Two hollow assertions, and what they were hiding

Both were written by the same hand that wrote the code, and both passed until they were
falsified — see [[tests-i-write-to-check-my-own-fix-are-hollow]].

- **AC2** searched the page for `"group_id"`, `"publish_period_secs"` and `"serial"`, which
  are the form's own input names and are present whether a fault was rendered or not. The
  green hid a real defect: the screen bound faults by `Fault::field` (a human label,
  `"publish period"`) instead of `Fault::source` (the key, `publish_period_secs`), so
  **nothing was ever shown beside its field** — which is exactly what AC2 asks for. Fixing
  the assertion is what exposed the code. `check_serial` carried no source at all and now
  does.
- **AC4** searched for `"in force now"`, which the change *table* prints on every hot row, so
  a mutation making the verdict above it say the opposite left the test green.

### A deployment trap the image smoke test found

The state directory is unwritable to uid 10002 unless it was `chown`ed — the documented step
everyone forgets, and Guy's own outstanding action on panoramix. Every earlier check only
*read* the state directory, so nothing had tried to write one. The bridge behaves correctly
(*"the configuration was NOT written… Nothing has changed"*, and it changes nothing), and that
behaviour now has its own check: a save that could not be written must never report success.

### Also added

A cross-binary **port lock**. A first run has no file to read a port from, so every test of an
unconfigured bridge wants `DEFAULT_PORT`, and `cargo test` runs test binaries in parallel.
Without it the loser reports "the UI never answered" — a flake that impersonates the very
defect these tests exist to catch.

### Not done here, deliberately

~~The runtime still serves **one** meter (`RUNTIME_METER_LIMIT`). Four meters publishing is
**Epic 3** (*The Full Fleet*), which the execution order places before Epics 5–6 and which has
been skipped; bringing it forward to make this story's testing more convenient was declined.~~

**No longer true, 2026-08-06 (story 3.1); struck 2026-08-08.** Epic 3 was opened right after this
story and `RUNTIME_METER_LIMIT` is gone — every enabled meter is served. The decision recorded
above was right when taken: the fan-out was done for its own reasons, not to make this story's
testing convenient. This is the last passage in the repository still asserting the one-meter
runtime in the present tense, and story 3.1's AC6 required a per-passage sweep that reached the
five it named and not this one.

---

## Review round — 2026-08-05, later

**Five fresh-context reviews (5.1, 5.2, 5.3, 6.1, 6.2), ~60 findings, all fixed.** Six
commits, +1452/−181. Full `ci-local.sh` green including the image; all three workflows green.
None of the review agents wrote to the tree — verified with `git status` after each.

**The story remains in `review`.** These corrections have been reviewed by nobody, which is
the same debt the round was run to clear.

### The four that mattered most, all introduced by this story

1. **The browser could brick the container.** Clearing the publish-period or broker-port box
   submits an empty string; `validate` reads it as unset and supplies its default, so the
   submission was accepted — and the writer re-derived the number from the raw string as
   `"".parse().ok().unwrap_or_default()`, which is **zero**. The page said "Saved", the file
   said `publish_period_secs = 0`, and the next start refused it. Story 6.1 AC1 serves no UI
   for an invalid file, so the operator was left with a crash-looping container and a
   hand-edit over SSH — in one click, through the supported path. **AC1's exact negation.**
   Fixed structurally: `StoredConfig` is derived `From<&BridgeConfig>`, so what reaches disk
   is what `validate` returned. The old function is `as_typed` and is for redisplay only.
2. **The configuration screen could be saved exactly once.** The always-rendered blank "Add a
   meter" row is submitted by a browser as three empty strings and was refused. Every edit
   after the first run needed a text editor — the thing this story exists to remove.
3. **A change that withdrew the confirmation was still carried to the wire.** `save` cleared
   `mapping_confirmed`; `apply` ran anyway, four lines later, on the same submission. FR25
   defeated through the screen built to enforce it.
4. **`Decision::Unconfirmed => {}`** — an automated substitution removed the wrong occurrence
   while cleaning up redundant stores, leaving for the third phase exactly the defect
   `b36f42d` had just fixed for the other two.

### Three hollow assertions in one file, and the third was my fix for the second

`contains("in force now")` was satisfied by the change table; I documented that, then wrote
`contains("broker_host")` — satisfied by the same table. Under it was a real gap:
`needs_restart()` omitted every `NewSession` field, which [#49] makes equally inapplicable.

**No test posted what the rendered form actually contains.** Every one hand-crafted a body
with `meter.0.*` only, which is why the one-shot form survived. The round-trip test exists now
and submits the page twice, because once proves only what was never broken.

### Also fixed, from the other four reviews

The cost table called a total silence *"one device re-announced"* (rename, serial change,
enabled-swap); `Control::apply` discarded its send results; `store::exists` reported a
permission error as *absent*; `save` wrote the caller's schema version and overwrote unreadable
files; two checks were skipped whenever another field was already wrong; duplicate faults named
neither offender; a hot period change made `/healthz` answer 503 about a healthy loop; *"The
bridge is connected and publishing"* was a compile-time constant; `origin::refuse` reflected raw
headers unescaped; the AC5 guard was untested on `/confirm`; an absence test proved nothing
because it never checked the process was alive; another was vacuous by construction; a restart
test was an `x == x`; `ci-local --fast` had stopped skipping what it promises.

ADR 0024 amended: the origin guard does **not** survive DNS rebinding, because it compares two
headers the same request supplies. What blocks that is Traefik's `Host(...)` rule and the
absence of a published host port — a deployment property Epic 7 must carry as a requirement.

**[#56] opened:** nothing inside the image can consume `/healthz` — no `curl`, no `wget`, and
the shell is not bash — so AC3 was falsified against nobody's implementation.

### Coverage note — 2026-08-20

**This story delivers FR46 and cites FR23, FR25 and FR28 instead.** `save_config` takes
`api_base`, `broker_host`, `broker_port`, `group_id`, `node_id`, `publish_period_secs`,
`log_dir`, `log_keep`, `ui_port` and the meter mapping — FR46's own enumeration, *"without
editing a file or restarting the container"*. Nothing is missing from the code; what was
missing is the claim, and FR46 was absent from the FR coverage map entirely (it entered the
PRD on 2026-08-03 with [ADR 0021] and both epics' scope lines, and never the map, which is
what coverage is measured against). Both are recorded now. Found by the review of stories
6.3–6.5, which had to ask what Epic 6 still owed before it could say what remained.
