# ADR 0026 — A configuration the bridge cannot use stops it publishing, not serving

- **Status:** accepted
- **Date:** 2026-08-06
- **Supersedes:** story 6.1 AC1's corner decision *("keep refusing")*, on the ground that the thing
  it lacked now exists. **Amends:** **FR26**.
- **Story:** 6.1 AC1, 6.2 · **Issue:** [#57](https://github.com/guycorbaz/smartme_mqtt/issues/57)

## Context

FR26 reads:

> The bridge can validate the full configuration at startup (topic uniqueness, well-formed
> serials, completeness) and **refuse to start on invalid config rather than start partially**.

Story 6.1 met it and refused to defer the awkward corner, deciding it at drafting time: *what
happens to the UI when the configuration is present and invalid?* The recorded answer was **(a)
keep refusing**, with three reasons — the refusal is FR26's whole point, the faults already reach
`docker compose logs`, and a bridge staying up on a configuration it rejected is one restart from
somebody believing it runs.

**That decision was right when it was taken, and it is now obsolete.** It was taken on 2026-08-04,
when the screens did not exist. The third reason — *somebody might believe it runs* — was a real
hazard precisely because there was no surface that could say otherwise. Story 6.2 built that
surface, and the correction round of 2026-08-05 built `Lifecycle::Misconfigured`, a page whose
headline is *"The saved configuration is not usable"* and which already answers `/healthz` with
`200` so that Epic 7's healthcheck cannot restart-loop it away.

So the position now costs what it was meant to buy. Concretely, four findings of the second review
round are one defect wearing four hats:

1. **The forgotten `chown`.** `/data` not traversable by uid 10002 makes `store::exists` return
   `true` (deliberately — an error must not read as *absent*, or an unreadable file gets
   overwritten), `read` then fails, the decision is `Invalid`, and `main.rs` returns `Err` **before
   the server is spawned**. Under a restart policy that is a loop with no screen, ever. This is the
   most likely first-run failure there is: the `chown` is a documented deployment step, which is to
   say it is one people forget. On the author's deployment it is worse than likely — [#41] showed
   the displayed mode disagreeing with the enforced Synology ACL, so the harsher branch is live.
2. **A hand-edited file with a syntax error.** FR23's headless bring-up invites hand-editing;
   a broken quote is what hand-editing produces. Cold start ⇒ no UI ⇒ the repair is a shell on the
   volume, which the deployment does not offer.
3. **The schema bump.** An image update that raises `SCHEMA_VERSION` refuses every existing file.
   ADR 0024 already records this as a live constraint on its own design.
4. **`store::save`'s refusal to overwrite an unreadable file** was widened on 2026-08-05 until the
   repair screen could not write the repair. Narrowed on 2026-08-06 — but it only ever helps on a
   *later* turn, because on the first turn there is no screen to save from.

The asymmetry is already half-argued in the code. `main.rs` declines to kill the process on a later
turn, and says why: *"killing the process would destroy the screen that is the repair tool."* That
sentence is true on the first turn too. Nothing in it depends on which turn it is.

## Decision

**A configuration the bridge cannot turn into settings stops it publishing. It does not stop it
serving.**

On every turn, including the first:

1. The process **starts**. It opens no MQTT session, publishes nothing, and emits no birth.
2. The UI is served, in `Lifecycle::Misconfigured`, and the faults are rendered into the form the
   operator repairs.
3. `/healthz` answers **`200`**, by story 6.1 AC3's own argument: a healthcheck that restarts the
   container destroys, every few seconds, the screen needed to fix it — and a restart provably
   cannot fix a configuration fault.
4. The faults keep going to `stderr` and to `docker compose logs`, unchanged.

**And "cannot be read" is reported distinctly from "was read and rejected"**, because the two have
different repairs and only one of them is in the browser:

| state | what the operator is told | where the repair is |
|---|---|---|
| the file could not be read at all (I/O, permissions) | the state directory, the uid, and the `chown` that fixes it | on the host |
| the file was read and rejected (syntax, unknown key, older schema) | each fault against the field it belongs to | in the form |
| the file came from a newer image | the version that owns it, and *roll the image forward* | in the deployment |

### Why this does not weaken FR26

FR26 names its own harm: **"rather than start partially"**. The harm is a bridge that publishes on
a configuration it only half understood — settings nobody chose, reaching a SCADA host that
persists them. That harm requires *having* settings and *using* them. Here there are none and
nothing is published; the bridge is exactly as silent as a refusal makes it.

What changes is that the silence is now **explicable from a browser** instead of only from a
terminal the deployment does not give you.

**FR26 is amended accordingly**: *refuse to start* becomes **refuse to publish, and say so on the
screen**. The validation, the fault set and the wording of every fault are untouched.

## Alternatives considered

- **Keep refusing (the status quo).** Rejected: it makes the single most common first-run mistake
  unrecoverable without host access, and the reason it was chosen — nobody could be told otherwise
  — has been fixed.
- **Refuse only on the first turn, as today, but exempt I/O errors.** This fixes the `chown` case
  and nothing else. Attractive because it is narrow; rejected because it leaves the hand-edited
  file and the schema bump exactly as stuck, and it would split one rule into two on a distinction
  (*which turn is it?*) that has no meaning to an operator.
- **Refuse, but write the faults to a file the operator can fetch.** A second surface with the same
  reach as the one that already exists.

## Consequences

- **`main.rs`'s `Decision::Invalid(_) if first_turn` arm goes away.** The two arms become one, and
  the `first_turn` flag may lose its last reader.
- **A new failure mode to keep honest:** the bridge is now *up* in a state where it does nothing.
  Everything that reports on it must say so — this is exactly the class of lie the correction round
  of 2026-08-05 found four times, and the reason `/healthz` carries `intends_to_publish`.
- **Epic 7's healthcheck** no longer has a path that restarts a container over a configuration
  fault. That was a hazard 6.1 identified for the *unconfigured* state and left open for the
  *invalid* one.
- **The manual changes in two places**: `06-operations-ui.tex`'s warning currently opens *"There is
  one state with no web interface at all"* — after this there is none. `04-configuration.tex`'s
  schema section must stop implying that a version mismatch is unrecoverable from a browser.
- **`store::save`'s narrowed guard (2026-08-06) becomes reachable on the first turn**, which is
  what makes the repair actually work rather than merely be allowed.

## What this ADR does not decide

**Whether the bridge should keep publishing on the configuration it started with, when the file on
disk later becomes unusable.** It does today, and that is untouched here. Note the page's current
wording for `Misconfigured` claims exactly that situation — *"The bridge is still running on the
configuration it started with"* — while the state is reachable only from `Unconfigured` and
`Unconfirmed`, where nothing was ever published. That wording is wrong today and must be fixed
whichever way this ADR goes; it is a defect, not a decision.

[#41]: https://github.com/guycorbaz/smartme_mqtt/issues/41
