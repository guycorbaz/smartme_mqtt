# ADR 0021 — Configuration is editable from the UI; Epic 5 owns the model, Epic 6 owns the screens

- **Status:** Accepted
- **Date:** 2026-08-03
- **Decided by:** Guy, on two questions put to him while drafting Epic 5's first story
- **Amended 2026-08-04** by [ADR 0023](0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md):
  **FR46 no longer covers the smart-me credentials**, which stay in the environment. Everything else
  this ADR decides — that configuration is editable from the UI at all, and the Epic 5 model /
  Epic 6 screens split — is unchanged. FR23 is rescoped a second time, to the credential alone.
- **Related:** **FR46** (new), FR23 (rescoped to bootstrap), **NFR14**,
  [ADR 0019](0019-no-auth-on-the-config-ui-secrets-are-write-only.md),
  [ADR 0020](0020-the-publish-period-is-bounded-and-cannot-be-turned-off.md), Epics 5 and 6
- **Changes a requirement**, which is why it is an ADR and not a story note. Tracked as
  [#47](https://github.com/guycorbaz/smartme_mqtt/issues/47).

## Context

ADRs 0019 and 0020 were written on 2026-08-03 to record decisions Guy took on 08-01. Both describe a
**form**: a secret submitted through the UI and stored write-only, a publish period that a UI field
must bound. Drafting Epic 5's first story against them surfaced that the thing they assume is owned
by nobody.

**The product description has always had it.** `prd.md:28` — the bridge is *"configured, previewed,
and diagnosed through a built-in web UI"*. `prd.md:110` — *"Web UI: config + live preview +
diagnostics"*. Journey 1 has Guy clicking *"Test connection"*. **NFR14** says *"credentials never
re-shown in clear"*, which is only meaningful if credentials can be entered.

**The requirement list did not.** FR23 said credentials arrive *"via environment/`.env`"*, and no FR
said the operator may change anything from a browser. FR24 and FR25 (mapping, first-run
confirmation) name no mechanism.

**The epic split did not either.** Epic 5 is described in terms of *"`.env` secrets discipline"*;
Epic 6 is *Observability, Diagnostics & the State-of-the-Bridge UI* — a read surface. So the config
UI fell between them.

This is the project's recurring defect in an unfamiliar direction: usually a claim changes and its
consequences are left behind. Here the *prose* was right all along and the **FR list — the artifact
coverage is measured against — was the stale one**. It would have been discovered by an epic-5
story asserting a behaviour no requirement authorised.

## Decision

### 1. Configuration is editable from the UI — FR46

New FR46: the operator can change and persist the meter mapping, the publish period, the broker
details and the smart-me credentials from the web UI, without editing a file or restarting.

**FR23 is not withdrawn; it is rescoped to the bootstrap path**, and it must stay sufficient on its
own. Two reasons, and neither is nostalgia: a bridge whose configuration can only be completed
through a browser cannot be brought up headless, and on a genuine first run there is no stored
configuration for a form to render.

### 2. Epic 5 owns the model; Epic 6 owns the screens

- **Epic 5 — Configuration, Secrets & Persistence.** What is configurable, its validation and
  refuse-to-start, its atomic persistence and hot reload, the bounded publish period, and the
  write-only storage of secrets. **All of it testable without a single line of HTML.**
- **Epic 6 — the UI.** The `axum` server and its bind, the configuration screens, the preview, the
  state screen, `/healthz`, the version display.

**Why this split and not the other.** The alternative — Epic 5 owning its own forms end to end —
gives each story a visible feature, but it drags the `axum` server into Epic 5, duplicating AR11,
and makes configuration depend on an HTTP stack. The decisive argument is the one that matters for
this project: **ADR 0019's write-only rule becomes falsifiable in Epic 5, before a form exists.** A
rule about what may leave the process is best tested at the boundary of the process, not at the
boundary of a page. Testing it only through rendered HTML would tie the guarantee to the shape of a
template.

## Consequences

- `prd.md` gains FR46 and the note on FR23. **Epic 5 and Epic 6's descriptions must be rewritten**,
  not annotated: Epic 5's *"`.env` secrets discipline"* framing is what hid the gap.
- Epic 6's FR list gains FR46's screen half; Epic 5 keeps FR23–27 and FR43.
- **The manual is deliberately NOT amended.** It documents behaviour that exists, and none of this
  does yet. The standing order is to update it on every behavioural change; a requirement is not
  one.
- The conformance matrix is untouched — this is a product requirement, not a Sparkplug clause.

## What this ADR does not do

It does not decide **where the configuration rests on disk**, nor in what format, nor whether
secrets share a file with the rest. That is architecture open item 5, still open, and ADR 0019
sharpened rather than settled it.

It does not decide whether a changed setting applies **without a restart**. Hot reload via `ArcSwap`
is AR8 and is Epic 5's to specify; FR46 says *"without restarting the container"*, which a reload
satisfies and a scheduled restart does not.
