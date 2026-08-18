# ADR 0037 — The first-run UI port is bootstrap, not configuration

- **Status:** accepted
- **Date:** 2026-08-18
- **Amends:** nothing. **Clarifies:** [ADR 0023](0023-the-file-is-the-configuration.md)'s boundary.
- **Issue:** [#93](https://github.com/guycorbaz/smartme_mqtt/issues/93)

## Context

A first run has **no `config.toml` to read a port from**, so the UI listens on
`ui::DEFAULT_PORT` — 8080 — and nothing else can be asked of it. That is correct in
production: the container's internal port is fixed and the mapping outside it is Docker's
business, which is why `docker-compose.yml`'s Traefik label, the image smoke test and four
manual passages all name 8080.

**On a development machine it is not correct, and the cost has now been measured twice.** On
2026-08-15 and again on 2026-08-18 the full gate was blocked because another project's
container held 8080; the second time it cost an hour across two attempts. Guy states the
condition plainly: *"d'autres développements sont en cours dans d'autres fenêtres"* — the
collision is the **normal** state of this machine, not an accident to wait out.

**`PortLock` cannot help and was never meant to.** It serialises this repository's own test
*binaries* against each other, through a lock file. It cannot evict a container.

**And moving `DEFAULT_PORT` is not available.** Measured on 2026-08-18: with the constant set
to 18080 the whole Rust suite passes — 38 binaries, 0 failures — and the **image smoke test
fails**, with `the image serves no web UI while unconfigured`. 8080 is a deployment contract,
not a default anyone is free to move.

## Decision

**1. The last-resort default may come from the environment: `SMARTME_UI_PORT`.** The
resolution order becomes, and the order is the whole decision:

1. `config.toml`'s `ui_port`, when there is a file — **unchanged, and still wins**;
2. otherwise `SMARTME_UI_PORT`, when it is set;
3. otherwise `ui::DEFAULT_PORT` (8080) — **unchanged**.

**2. This does not weaken ADR 0023, and the precedent is already in the tree.**
`SMARTME_STATE_DIR` has lived in the environment since story 5.1 without anyone calling it a
violation, because it says **where the configuration lives**, not **what it says**.
`SMARTME_UI_PORT` is in that family: it is consumed *only* on the path where no configuration
exists, and the moment a file exists the file decides. ADR 0023's sentence — *the file is the
configuration* — is untouched, because this variable is never read when there is a file.

**3. A malformed or privileged value refuses the start**, with the same rule `config.rs`
applies to `ui_port` from the file: `0` and `1..1024` are refused. A value that cannot be
honoured must not fall back silently to 8080 — that would put the UI somewhere the operator did
not ask for, and the operator would go looking for it where they did.

**4. The image never sets it.** No `ENV` line, nothing in `docker-compose.yml`. A deployment
that does not set it behaves exactly as it does today, which is why the smoke test, the Traefik
label and the manual's four passages need no change beyond documenting that the variable
exists.

## Consequences

### What this buys

The two first-run tests stop being hostages to whatever else is running on the machine. They
are the only two that must bind the default — every other test writes a `config.toml` with its
own `ui_port` — and they now take their port from the environment like the state directory.

### What it costs, and it is real

**A second environment variable, on a project that deliberately has almost none.** Each one is
a place a deployment can differ from its file, and the argument in §2 is a boundary, not a
wall: the next reader who wants "just one more" will cite this ADR. The boundary is stated so
it can be enforced — **anything the operator chooses belongs in the file; only what tells the
bridge where to find the file, or what to do when there is none, may live in the environment.**

### What it does not fix

The recurring collision on a shared development machine is *worked around*, not removed. If a
future test must genuinely observe the production default, it will collide again, and the
answer then is to run it where nothing else listens.

## What would reopen this

A deployment needing to set the UI port without a `config.toml`. That would make this variable
operator-facing rather than bootstrap, and it would then belong in the file — which is where
ADR 0023 already put it.
