# Story 6.2: The configuration screen — and the click that ends the silence

Status: ready-for-dev

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

## Tasks / Subtasks

- [ ] **Task 1 — read before writing**
  - [ ] `app::supervisor::Control` — the whole API, and why `current()` reports what is IN FORCE.
  - [ ] `app::reconfigure::Plan` — `cost()`, `needs_restart()`, and the four costs.
  - [ ] `app::config::{validate, Fault, Source, mapping_preview}`.
  - [ ] `app::store::{save, confirm}` — and **why `confirm` bypasses `save`**.
  - [ ] `ui/mod.rs` — `Lifecycle` is the single source of truth; do not add a second.

- [ ] **Task 2 — the form** (AC: 1, 2, 6)
  - [ ] Server-rendered, no SPA (`architecture.md:217`). `axum`'s `Form` extractor.
  - [ ] Faults rendered from `ConfigErrors`, never re-derived.
  - [ ] No credential field. Assert its absence from the response body.

- [ ] **Task 3 — confirmation** (AC: 3)
  - [ ] Its own route and its own submission.
  - [ ] The table shows serial, device id and the exact topic, from `mapping_preview`.

- [ ] **Task 4 — cost reporting** (AC: 4)
  - [ ] `Plan::cost()` rendered in the operator's terms.
  - [ ] **Re-read `Control::current()` after applying** and render THAT, not the submission.

- [ ] **Task 5 — same-origin** (AC: 5)
  - [ ] The chosen mechanism, applied to every mutating route, with the decision written down.
  - [ ] A test that a cross-origin POST is refused **and** that a same-origin one is not — the
        second half matters, or a guard that refuses everything would pass.

- [ ] **Task 6 — falsification** (AC: all)
  - [ ] Accept a period outside ADR 0020's bounds through the form and confirm the test catches it
        — that is the "the form validates too" defect.
  - [ ] Report a new-session change as in force and confirm the test catches it.
  - [ ] Fold confirmation into the save and confirm the test catches it.
  - [ ] **Assert every mutation actually applied before running it.** On 2026-08-04 a mutation
        matched nothing because `rustfmt` had reflowed the target, and the test stayed green.
  - [ ] `./scripts/ci-local.sh`, **full**.

- [ ] **Task 7 — the consequences**
  - [ ] `docker-smoke.sh`: a first run can be configured over HTTP end to end.
  - [ ] The manual's chapter 6 is a stub; it gains the screens.
  - [ ] NFR11 — *time-to-first-value under 15 minutes from a clean machine* — becomes measurable for
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
