# ADR 0023 — The stored file is the configuration; the smart-me credential never leaves the environment

- **Status:** Accepted
- **Date:** 2026-08-04
- **Decided by:** Guy — *"la clef api doit être dans une variable d'environnement. Tout le reste doit
  être configuré par une interface web. Le fichier est la configuration."* The pairing of
  `client_id` with the secret (below) was left to Claude with an instruction to decide and record it.
- **Supersedes** [ADR 0022](0022-secrets-rest-in-a-separate-0600-file.md) in full. There is no
  `secrets.toml`.
- **Related:** **FR23** *(rescoped again, see Consequences)*, **FR27**, **FR46** *(amended — its
  credentials clause is withdrawn)*, **NFR12** *(restored, see below)*, **NFR14**,
  [ADR 0019](0019-no-auth-on-the-config-ui-secrets-are-write-only.md),
  [ADR 0021](0021-configuration-is-editable-from-the-ui.md), Story 5.2,
  [#41](https://github.com/guycorbaz/smartme_mqtt/issues/41), [#48](https://github.com/guycorbaz/smartme_mqtt/issues/48)

## Context

Story 5.2 stalled on a policy, not on code. Both halves were written and tested — `store::load`
returns the same `RawConfig` the environment produces, and a single `app::config::validate` governs
both — but nothing said which source wins on a boot where `config.toml` and the `SMARTME_*`
variables both exist and disagree. `store::load` was therefore never wired into `main.rs`
(`app/store.rs:165` had no caller outside its own tests).

### The specification was already in three-way disagreement

This is the part worth recording, because the decision below is not a change of direction. It is the
resolution of a contradiction that had been sitting in the planning artefacts for a day:

| passage | says |
| --- | --- |
| **NFR12** (`prd.md:354`, `epics.md:107`) | *"Credentials only in `.env`/env vars, perms 0600, never in the image"* |
| **architecture.md:204** | *"Secrets (item ⑤): `.env` / env vars only … **The UI never reads, writes, or re-displays secrets** — only non-secret config."* |
| **FR46** (`prd.md:299`, added 2026-08-03) | the operator can change *"the meter mapping, the publish period, the broker details **and the smart-me credentials**"* from the UI |
| **ADR 0022 + architecture.md:121** (2026-08-03) | the credential rests in a `0600` `secrets.toml` in the state directory |

ADR 0022 was written against FR46 and never looked at NFR12 or `architecture.md:204`, both of which
it contradicts outright. `architecture.md` consequently contradicted **itself** — item ⑤ resolved
one way at line 121 and the opposite way at line 204 — for the second time in two days, the first
being the `restart-only` / `ArcSwap` clash that writing Story 5.2 uncovered.

**This decision restores NFR12 and `architecture.md:204`.** FR46 and ADR 0022 are the passages that
move.

## Decision

### 1. One file, and it is the configuration

`config.toml` in the state directory holds every setting. There is no second file. What it does not
hold, the environment does not supply either — a setting is in the file or it does not exist.

### 2. The environment carries exactly two things

| variable | why it cannot be in the file |
| --- | --- |
| `SMARTME_STATE_DIR` | it is *where the file lives*; a file cannot say where it is |
| `SMARTME_CLIENT_ID` + `SMARTME_CLIENT_SECRET` | this decision — the credential never descends to disk |

Every other `SMARTME_*` variable is withdrawn: `API_BASE`, `GROUP_ID`, `NODE_ID`, `BROKER_HOST`,
`BROKER_PORT`, `PUBLISH_PERIOD_SECS`, `METER_ID`, `DEVICE_ID`, `SERIAL`, `LOG_DIR`, `LOG_KEEP`.
Eleven, all of them now settings in the file, editable from the UI.

### 3. `client_id` travels with the secret, as one credential

`client_id` is not itself sensitive and could have lived in the file. It does not, for one reason:
a credential is a pair, and a pair split across two sources can be rotated by halves. The failure
mode of a mismatched pair is an authentication rejection from the smart-me API — which presents as
an outage of the upstream service, not as a configuration fault, and is diagnosed accordingly and at
length. One credential, one source, rotated in one place.

The cost is that changing the smart-me account means editing `.env` and restarting, where every
other setting is a form and a hot swap. That is accepted: an account changes approximately never,
and [ADR 0009](0009-smartme-auth-client-credentials.md) already put both halves in `.env`.

### 4. The domains are disjoint, so there is no precedence rule

No field is readable from both sources. There is therefore no "which wins" to arbitrate, no merge,
and no case where a value set in one place is silently overridden by the other.

That is the property being bought, and it is worth naming: **every precedence rule is a place where
one source loses without saying so.** Had the environment won, a period changed in the browser would
have reverted at the next restart to a stale `.env` on the deployment, with no fault and no trace.
Had the file won, an operator editing `.env` would have seen nothing happen. Disjoint domains make
both impossible by construction rather than by care.

### 5. A first run starts, serves the UI, and puts nothing on the wire

There is no `config.toml` on a fresh deployment, and everything but the credential arrives through a
browser. The bridge must therefore come up **without** a configuration — otherwise the screen that
creates one is unreachable.

- **no configuration at all** → the process starts, serves the web UI, and opens **no MQTT session**:
  no CONNECT, no will registered, no NBIRTH, nothing published until a configuration exists;
- **a configuration present but invalid** → refusal to start, exactly as Story 5.1 specifies.

Story 5.1 is not weakened. It governs a configuration that exists; **absence is not invalidity.**

## Consequences

### On ADR 0022 — superseded entirely

`secrets.toml`, its `0600` creation, the startup `stat` that verified the mode, and the cross-file
desynchronisation fault all lose their object. Nothing sensitive rests in the state directory.

In the code committed at `6476412`, this makes `StoredSecrets`, `secrets_path`, `check_mode` and
`persist_atomic_with_mode` dead, along with three of `store.rs`'s six tests. **`RawConfig`'s
hand-written `Debug` stays** — the secret still transits that struct on its way in from the
environment, which is exactly where the leak of `1-6` happened.

### On [#41] — it stops blocking, and changes species

[#41] blocked Story 5.2 because *"a `0600` file inside a directory anyone can write to is a claim
about one inode in a directory where files can be replaced."* With no secret at rest, the
**confidentiality** half of that argument dissolves.

The **integrity** half does not: whoever can write `/data` can replace `config.toml` and point the
bridge at another broker. That is a real and lesser risk, it is no longer anyone's prerequisite, and
it is stated here rather than quietly dropped. **Story 5.2 is unblocked.** [#41] reverts to an
ordinary deployment task.

### On ADR 0019 — the decision stands, one clause loses its subject

No authentication on the UI: unchanged. But ADR 0019's write-only rule was built on secrets being
*submitted through the UI and stored*; none now are. The *never rendered* clause survives as a
guard rather than a feature — there is no longer anything for a form to render.

### On the requirements

- **FR46** drops *"and the smart-me credentials"*. Everything else it promises is unaffected.
- **FR23** is rescoped a second time. It currently says the environment path *"must remain
  sufficient on its own: a bridge whose configuration can only be completed through a browser cannot
  be brought up headless."* That claim does not survive this decision — but the need behind it does,
  and it is met differently: **a headless bring-up writes `config.toml` by hand.** It is a
  documented TOML file with a versioned schema, which is a better headless surface than eleven
  environment variables were. FR23 keeps the credential half and hands the rest to the file.
- **NFR12** needs no change. It has been right since the beginning and is what ADR 0022 walked past.

### Elsewhere

- Backups and `docker cp` of the data volume **no longer carry a credential**. ADR 0022 said they
  would and made the manual owe a warning; that debt is cancelled rather than paid.
- The operator manual's chapter 4 already documents secrets as living in `.env` at `0600`. It was
  right and stays right. The eleven withdrawn variables must leave it when they leave the code —
  the manual documents behaviour that exists.

## What this ADR does not decide

**Whether `config.toml` deserves integrity protection.** It is not signed and not checksummed.
Directory permissions are the whole of the protection, which is the remaining half of [#41], stated
rather than implied. If the state directory is ever shared, this should be re-weighed.

**How the UI behaves on a first run.** The screens are Epic 6 ([ADR 0021](0021-configuration-is-editable-from-the-ui.md)).
This ADR fixes only that the process comes up, stays up, and stays off the wire in that state.
