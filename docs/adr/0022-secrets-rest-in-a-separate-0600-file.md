# ADR 0022 — Secrets rest in a separate `0600` file, and the bridge verifies the mode rather than assuming it

- **Status:** Accepted. **Carries an operational prerequisite that is not yet met** — see
  *The blocker*.
- **Date:** 2026-08-03
- **Decided by:** Guy
- **Closes:** architecture **open item 5** — *"Broker/token secrets-at-rest boundary when stored via
  the config file (same-file vs separate / env / `0600` / Docker secret)"* — open since the
  architecture was written.
- **Related:** **FR46** (configuration editable from the UI), **NFR14**,
  [ADR 0019](0019-no-auth-on-the-config-ui-secrets-are-write-only.md) (write-only, the outbound
  half), [ADR 0021](0021-configuration-is-editable-from-the-ui.md), Story 5.2,
  [#41](https://github.com/guycorbaz/smartme_mqtt/issues/41)

## Context

FR46 puts the smart-me credentials behind a form, so they must rest somewhere the process can write.
ADR 0019 settled what may **leave** the process — nothing, ever: secrets are write-only, never
rendered, never traced. It said nothing about what the value rests on, and explicitly left that to
this item.

The bridge already has an atomic writer, `persist_atomic` (`persist.rs:17`), which serialises TOML
through temp-file + `fsync` + `rename` + `fsync(dir)`. It writes into the state directory —
`SMARTME_STATE_DIR`, defaulting to `/data` (`main.rs:233`).

**And `/data` on the deployment is world-writable today.** That is [#41], Guy's deliberate stopgap.
The issue also records the fact that makes this decision urgent rather than tidy: **the mode bits
read `drwxrwxrwx` while a Synology ACL was denying uid 10002 access.** The displayed mode was not
the enforced permission. Writing a client secret into that directory without deciding anything would
have been an exposure adopted by accident.

## Decision

**Two files, split by sensitivity.**

| path | mode | contents |
| --- | --- | --- |
| `config.toml` | `0644` | meters, publish period, broker host and port, node and group identity |
| `secrets.toml` | `0600` | smart-me client secret, broker password if one is ever set |
| `bdseq.toml` | `0644` | unchanged, existing |

**And the bridge verifies the mode at startup rather than trusting the mode it set.** If
`secrets.toml` is readable by group or other, the bridge **refuses to start** and names the file.

### Why verify rather than assume

Because [#41] already produced a case where the permission a human read was not the permission in
force. A `create_new` with mode `0600` establishes what *this* process intended; it says nothing
about what a restore, a volume remount, an `umask`, a `docker cp`, or a NAS-side ACL did afterwards.
The check costs one `stat` per boot and converts a silent exposure into a refusal to start — the
same trade FR26 makes for every other kind of invalid configuration.

### Why two files rather than one

The alternative — everything in one `0600` file — is simpler, atomic in a single write, and cannot
desynchronise. It was rejected for one reason: it makes the **whole** configuration unreadable
without privileges, including for diagnosis where nothing sensitive is at stake. Checking which
topic a meter maps to should not require reading a file that also holds a credential, because the
habit that forms is `sudo cat`, and the habit that follows is a credential on a screen during a
support conversation. See the standing rule that a mask is a claim about output format: the way to
not leak a secret is for it not to be in the document.

The cost is accepted explicitly: **two files can desynchronise.** Story 5.2 owns that — a meter
present in one and absent from the other is a validation fault like any other, not a panic.

## Consequences

- **Story 5.2 cannot be considered done while `/data` is world-writable.** This ADR is accepted; its
  prerequisite is not met. See below.
- `persist_atomic` needs a mode-aware sibling, or a documented `set_permissions` immediately after
  creation and **before** the first write of a secret. Creating `0644` and tightening afterwards
  leaves a window; the window is short and real.
- Backups and `docker cp` of the data volume now carry a credential. The operator manual must say so
  where it describes the volume — the manual documents behaviour that exists, so this lands when 5.2
  does, not before.
- A secret that is *removed* from the UI must be removed from `secrets.toml`, not left orphaned. The
  write-only rule means the UI cannot show what is there, so a stale value would be invisible.

## The blocker — and it is Guy's, not the code's

**[#41] must be closed first.** `/data` on panoramix is world-writable, and the mode bits there have
already been shown to disagree with the enforced ACL. Until the directory itself is sound, a `0600`
file inside it is a claim about one inode in a directory anyone can write to — and a directory
anyone can write to is a directory where a file can be replaced.

This ADR deliberately does **not** specify how to fix the volume — uid, ACL, or share settings are
deployment facts, and the repository is public. It specifies only that Story 5.2's acceptance
depends on it, so the dependency is visible rather than discovered at deployment.

## What this ADR does not decide

Whether secrets are **encrypted at rest**. They are not, and this ADR does not pretend otherwise:
file permissions are the whole of the protection. Encryption would need a key, the key would need to
rest somewhere, and on a single-user homelab with no HSM that is a longer chain with no clear
anchor. If that changes — a shared NAS, a backup leaving the house — this decision should be
re-weighed rather than assumed to still hold.
