# Story 7.2: What Epic 7 already delivered, proved rather than assumed

Status: done

> **This story exists because of Epic 6's action F3.** That epic looked like it owed four
> requirements and owed two: FR46 was delivered by story 6.2 and never written into the
> coverage map, FR38 by `main.rs` and claimed by no story at all. Epic 7 is in the same
> position — the `Dockerfile`, the compose file with Traefik, and the publish workflow all
> exist and **no story claims any of them**.
>
> **So the work is not to build them. It is to prove them, and to write the proof that is
> missing.** One is: FR40 says *"update by pulling a new image without config loss"*, and every
> test of it operates on the **file**, not the image. Nobody has ever observed a container come
> up on a volume a previous container configured.

## Story

As the author about to deploy this bridge for real,
I want each of Epic 7's requirements tied to something that runs,
so that "delivered" and "proved" are the same claim rather than two.

## Acceptance Criteria

**AC1 — a table, one row per requirement, and every row names its proof.**

**Given** FR39, FR40, NFR21, AR11, AR12 and AR22
**When** the epic is read
**Then** each has a row naming the artefact that delivers it **and the test or gate that
exercises it**
**And** a row whose proof is *"read the file and see"* is written as **unproven**, in those
words — the conformance matrix's own convention from Epic 4.

**AC2 — FR40 is observed on a container, not simulated on a file.**

**Given** a state directory a container has already configured
**When** a **new container** is started on the same volume
**Then** it comes up publishing, with the mapping still confirmed and every setting intact
**And** it writes no configuration of its own — the run that finds a file must not touch it
**And** this replaces "we tested the file round-trips" as FR40's evidence, which is a claim
about `serde`, not about an update.

**AC3 — the non-Traefik fallback is valid, not just written down.**

**Given** the commented `ports:` block the compose file offers for deployments without Traefik
**When** it is used
**Then** the resulting compose file is valid and binds the port it says
**And** if it is not checked mechanically, it is recorded as unproven rather than described as
supported.

**AC4 — the image is what NFR21 says it is.**

**Given** NFR21's *"single-arch Docker Hub image with a Docker healthcheck"*
**When** the workflow and the image are read
**Then** the platform is named and singular, and the healthcheck is present — the second half
is story 7.1's and is cited, not re-proved.

**AC5 — falsification.**

**Given** each new proof
**When** the mechanism it names is broken
**Then** it goes red, and the run's output is copied next to it.

## Out of scope

- **Multi-arch images.** NFR21 says single-arch and the workflow builds `linux/amd64`. Adding
  `arm64` is a decision with a build-time cost, not a gap in this epic.
- **Rollback.** FR41 is Epic 8's, and it is a documentation requirement with a procedure, not a
  mechanism here.
- **Traefik itself.** Standing up a Traefik instance in CI proves somebody else's software. What
  is checkable here is that the labels are well-formed and the network posture is what the file
  claims.

## Dev Notes

### What must not break

- **The smoke test's own rule**: presence before verdict. A check that reads a value from a
  container must first prove the value exists, or "not wrong" passes against "not there".
- **`docker-smoke.sh` runs in CI and in `ci-local.sh`** — anything added must be as
  self-cleaning as what is there (named containers, `docker rm -f` in every path).
- **No new package in the image** (ADR 0041's reasoning still holds).

### References

- [Source: `_bmad-output/planning-artifacts/epics.md`] — Epic 7's scope line and its FR/NFR/AR list
- [Source: `_bmad-output/implementation-artifacts/epic-6-retro-2026-08-20.md`] — action F3, which this story is
- [Source: `scripts/docker-smoke.sh`] — where a claim about the image can actually be checked
- [Source: `crates/smartme-bridge/tests/config_survives_an_image_update.rs`] — what FR40's evidence is today, and why it is not enough
- [Source: `CLAUDE.md`] — falsify before trusting

## The table AC1 asks for

*Every row names what delivers the requirement **and** what exercises it. A row whose only
proof would be "read the file and see" says **unproven**, in Epic 4's convention.*

| Requirement | Delivered by | Exercised by |
|---|---|---|
| **FR39** — deploy and start via `docker compose` | `docker-compose.yml` (Traefik labels, the `proxy` network, no host port; a commented `ports:` fallback) | `ci-local.sh` parses it **as shipped and with the fallback uncommented** (this story). Traefik *routing* is **unproven**, deliberately: standing up Traefik in a gate proves somebody else's software |
| **FR40** — update by pulling a new image, no config loss | the state directory being a volume, and `save`'s refusal to touch a file it did not change | `docker-smoke.sh`: a **new container on the volume a previous one configured** comes up publishing and rewrites nothing (this story). The file-level round-trip tests remain, one layer down |
| **NFR21** — single-arch image with a Docker healthcheck; updates preserve config | `docker-publish.yml` (`platforms: linux/amd64`), the `HEALTHCHECK` line | the platform is **read from the workflow, not asserted** — *unproven*, and cheap to leave so: a second platform would fail the build it was added to. The healthcheck is story 7.1's, and the image reports `healthy` in `docker-smoke.sh` |
| **AR11** — compose / Traefik / Dockerfile | all three files | the image runs as uid 10002 and carries a CA bundle (`docker-smoke.sh`, since Epic 0); the compose file parses (this story) |
| **AR12** — healthcheck wiring | `HEALTHCHECK` → `--healthcheck` | story 7.1: the probe's verdict on 200, on 503 and on silence; the image reports `healthy` |
| **AR22** — CI/CD building and pushing the image | `docker-publish.yml`, tags `v*` | the workflow runs on every push (build, no push) and on tags (push). **The push path is exercised by having been used**: `v0.4.0-rc3` is the image `docker-compose.yml` pins |

## Dev Agent Record

### Agent Model Used

Claude Opus 5 (1M context) — `claude-opus-5[1m]`, 2026-08-20.

### Completion Notes List

**AC1 — met, and two rows say *unproven* rather than being dressed up.** Traefik routing and
the image's platform are both read rather than asserted, and the table says so in Epic 4's
convention. Writing "covered" there would have been the epic-6 defect one level up.

**AC2 — met, and it is the row that changes what is known.** FR40's evidence was a struct
written, read back and compared — a claim about `serde`. `docker-smoke.sh` now starts a
**second container on the volume the first one configured**, and asserts two things: it comes
up publishing, and the configuration's checksum is unchanged.

**What that check does and does not detect, stated because the first mutation revealed it.**
A rewrite that produces *identical bytes* is invisible to it. That is the honest boundary: what
the criterion protects is the operator's configuration not being edited, and a byte-identical
rewrite edits nothing. The mutation that dropped an absent optional key was therefore a no-op
and passed — recorded here rather than quietly re-run until something failed.

**AC3 — met.** The fallback is uncommented mechanically and parsed. **Presence before verdict**:
if the `sed` matches nothing, both files are identical and the parse would prove nothing, so the
check refuses when it cannot find `ports:` in its own output.

**AC4 — met by citation.** `platforms: linux/amd64`, singular, in the workflow; the healthcheck
is 7.1's and is not re-proved here.

### Falsification record

| # | Mutation | Went red with |
|---|---|---|
| 1 | startup rewrites the configuration with an **out-of-range** period | `FR40: a new container on an already-configured volume did not come up publishing` — the rewrite made the file invalid, and the first assertion caught it |
| 2 | startup rewrites it with a **legal** value (60 s) | `FR40: the second container REWROTE the configuration (e786384…→ 201ab4a…)` — the checksum assertion, which is the one this mutation was written for |
| 3 | the commented `ports:` block given an extra space, so the `sed` no longer matches it | `the non-Traefik fallback does not parse` — the presence guard fired through its consequence |
| — | *(no-op, recorded)* startup rewrites it dropping `log_keep`, which the fixture never set | **nothing went red**, and that is the boundary above |

### File List

- `scripts/docker-smoke.sh` — modified (FR40 on a container)
- `scripts/ci-local.sh` — modified (the reference deployment parses, both ways)
- `.gitignore` — modified (the fallback scratch file)
- `_bmad-output/implementation-artifacts/7-2-…md` — new
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — modified

### Change Log

- **2026-08-20** — Story 7.2. Epic 6's action F3, applied to Epic 7's own inheritance. Three
  mutations run and one no-op recorded. No production code changed: this story adds proof, not
  behaviour.

### Review — 2026-08-20

**One defect, introduced by this story into the gate itself.** The compose check `export`ed
`SMARTME_CLIENT_ID` and `SMARTME_CLIENT_SECRET` to satisfy Compose's `:?` form — and an
`export` in `ci-local.sh` leaves them set for **every step below it, the whole Rust suite
included**. This repository has already paid for that shape once: story 6.6's own test had to
be made immune to those variables because other tests in the same binary set them and the
environment is per-process. **A gate that quietly hands the suite a credential changes what it
is measuring.** Repaired: the values are supplied on the command, never exported.

**And one silence removed.** The block was wrapped in `if command -v docker`, so on a machine
without Docker it produced no output at all — and a check that vanishes reads afterwards
exactly like a check that passed. It now prints that it was skipped.

**The rest holds.** The two `unproven` rows are still honest, the FR40 container check cleans
up its container on every path, and the presence guard on the fallback fires through its
consequence.
