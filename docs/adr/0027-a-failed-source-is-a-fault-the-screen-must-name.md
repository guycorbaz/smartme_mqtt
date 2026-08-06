# ADR 0027 — A failed source is a fault the screen must name, and a restart cannot clear

- **Status:** accepted
- **Date:** 2026-08-06
- **Supersedes:** nothing. **Amends:** [ADR 0009](0009-smartme-auth-client-credentials.md)'s *"stop +
  surface"*, by deciding what *surface* means now that there is one.
- **Story:** owns a new Epic 6 story; the republish half belongs to **Epic 2** · **Issue:**
  [#58](https://github.com/guycorbaz/smartme_mqtt/issues/58)

## Context

This project exists to prevent one thing: a SCADA host displaying a value as trustworthy when it is
not. The second review round found that failure, reachable by a single typo, on the deployment path
that runs tomorrow.

**Enter a wrong `SMARTME_CLIENT_ID` or `SMARTME_CLIENT_SECRET` and the bridge reports itself
healthy while putting nothing on the wire, indefinitely.** Every step is working as designed, which
is why 167 green tests do not see it:

- `State::Failed` is **absorbing** by construction (`core/state_machine.rs`, ADR 0009): a fatal
  error means retrying with the same configuration would keep lying, so only a fresh process can
  leave it. Correct.
- `poll_publish` sends a `MeterUpdate` **only when the fetch succeeded**. On a failure it traces the
  verdict and sends nothing. The comment there says the mqtt task *"republishes the last known value
  with this quality"* and names its owner in the same breath: **`Epic 2 wires the republish`**.
- The only site that publishes DDATA is fed by that channel. No update, no DDATA — ever.
- No DDEATH either: device certificates are emitted **only** by `Control::apply`, from a
  configuration change that disables a meter. Nothing in the freshness path emits one.
- Meanwhile the poll loop keeps turning, so `LastLoopTick` is touched every period, `wedged` stays
  `false`, `/healthz` answers `200` with `intends_to_publish: true`, and `/` says the bridge *"is
  polling the meters and publishing what it reads"*.

So: the node births, the host acquires and persists the tag folder, and then receives nothing, with
no death certificate and no quality change. **The last view the host holds is the one it keeps.**
Only `docker compose logs` disagrees, and only in the state the bridge is least likely to be watched.

Epic 2 — *Exhaustive "Never Lies" Oracles & Freshness Hardening* — owns the missing republish. It
has **no entry in `sprint-status.yaml` and not one story written**. The mechanism that enforces the
product's central claim is parked in an epic nobody has planned.

## Decision

Three things, and they are separable — the first is buildable now, the second is a rule, the third
names an owner.

### 1. A meter's oracle state reaches the UI, and the screen never claims more than it can see

`Lifecycle::Running`'s text — *"polling the meters and publishing what it reads"* — is a claim about
the **source**, and today the UI has no access to the source's verdict. It gets one. A bridge whose
meters are all `Failed` is not described as publishing; the page names the meter, the fault, and
that a restart is required.

This is the honesty half of FR28 (*each meter's live value, unit, freshness age, target topic,
serial, and published status*) pulled forward ahead of the rest of it, because the claim the page
already makes is the one that has to become true first.

### 2. `/healthz` answers `200`, and the reason is not comfort

A `Failed` source is a **fault**, not a deliberate silence, so `is_silent_on_purpose` must not
absorb it — the JSON has to distinguish the two. But the status code stays `200`, for the reason
story 6.1 AC3 gave and [ADR 0026](0026-a-configuration-it-cannot-use-stops-the-bridge-publishing-not-serving.md)
reuses: Epic 7 wires this endpoint to a container restart, and **a restart provably cannot clear a
rejected credential.** It would loop, destroying the screen that names the fault, every few seconds.

The healthcheck's job is to restart a *wedged poller*. A poller that is running and being refused is
not wedged; it is being answered, and the answer is no.

**A precision worth recording**, because the code's own comment is now half-stale: `state_machine`
justifies the absorbing `Failed` with *"config is restart-only"*. Since story 5.2's hot reload that
is no longer true in general — but it remains true for **the credential specifically**, which
[ADR 0023](0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md) keeps in
the environment. The justification survives; its stated reason needs narrowing.

### 3. The republish with a non-good quality is required, and it is Epic 2's

Silence on a Sparkplug wire is not a statement. It is indistinguishable from *"nothing has changed"*,
which is why a host goes on displaying what it last received. **A verdict the bridge has reached and
does not publish is a verdict it has withheld**, and withholding it is the failure this project is
named for.

The rule: **every poll cycle produces a published verdict for every enabled meter — a value with its
quality, or no value with a non-good quality — or it produces a device certificate.** Not silence.

This ADR does not implement it: the mechanism is Epic 2's and touches the state machine, the channel
and the publisher. It decides **that it is required**, so that the epic is written against a
requirement rather than discovering one.

## Alternatives considered

- **`/healthz` reports unhealthy on `Failed`.** The honest-looking answer, and wrong: Epic 7 turns it
  into a restart loop over a fault a restart cannot touch, and the loop eats the diagnosis.
- **Leave the screen alone and rely on the logs.** This is the status quo, and it is what made the
  defect survive a full review round. The logs are the surface an operator reaches *after* they
  suspect something; the page is what tells them to suspect it.
- **Make `Failed` non-absorbing so a later success clears it.** Rejected, and it is not a close call:
  ADR 0009 chose absorbing so that a transient success cannot launder a broken configuration into
  apparent health. That reasoning is intact.

## Consequences

- **A new Epic 6 story** for the source verdict on the screen, ahead of the rest of FR28.
- **Epic 2 acquires a written requirement** and must be planned before the fleet grows: with four
  meters, three silences hide behind one that works. This is the hinge between Epic 2 and Epic 3 —
  per-meter freshness isolation (NFR2) is worth nothing while a silent meter publishes nothing at
  all.
- **`poll_publish`'s comment stops being a promise and becomes a reference** to this ADR and to the
  epic that owns it.
- **A test that would have caught this** is owed: drive the poll loop with a source that always
  fails, and assert on what reaches the wire and on what the page says. Note the trap — this is an
  absence assertion, and this repository has shipped several that held over an empty stream. The
  stream must be proved to flow first.
