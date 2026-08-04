#!/usr/bin/env bash
# Run exactly what CI runs, before pushing.
#
# This exists because "I tested locally" is not the same claim as "CI will
# pass", and on 2026-07-26 the difference cost six red commits. The isolated
# workflow builds with `--locked`, which fails on an uncommitted Cargo.lock —
# a case no amount of `cargo test` locally will ever reproduce.
#
# The commands below are copied verbatim from .github/workflows/. If you change
# a workflow, change this too; they are meant to be compared line by line.
#
# THIS FILE REPRODUCED TWO WORKFLOWS OUT OF THREE UNTIL 2026-08-04, while saying
# it reproduced CI. The Docker one was added later and never landed here, so a
# change to how the bridge STARTS left an image smoke test hanging for 26
# minutes against a 3 minute baseline — with every local run green. Its checks
# now live in `scripts/docker-smoke.sh`, called by both, because two copies of a
# check drift and one copy cannot.
#
#   ./scripts/ci-local.sh          full run
#   ./scripts/ci-local.sh --fast   skip the chaos tests (no Docker needed)
#
# Chaos tests need a Docker daemon (testcontainers starts a Mosquitto broker).

set -euo pipefail
cd "$(dirname "$0")/.."

fast=0
[[ "${1:-}" == "--fast" ]] && fast=1

step() { printf '\n\033[1m── %s\033[0m\n' "$1"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$1"; }

# ---------------------------------------------------------------------------
# Not a CI step, but the failure mode CI cannot warn you about early enough:
# a dependency change that never made it into the commit.
# ---------------------------------------------------------------------------
step "Cargo.lock is in sync with the manifests"
if ! git diff --quiet -- Cargo.lock; then
    echo "Cargo.lock has uncommitted changes — stage it, or the --locked build below fails in CI:"
    git --no-pager diff --stat -- Cargo.lock
    exit 1
fi
cargo metadata --locked --format-version 1 >/dev/null
ok "lock file matches the manifests and is committed"

# ---------------------------------------------------------------------------
# .github/workflows/ci.yml
# ---------------------------------------------------------------------------
step "ci.yml — fmt"
cargo fmt --all --check
ok "fmt"

step "ci.yml — clippy"
cargo clippy --workspace --all-targets -- -D warnings
ok "clippy -D warnings"

step "ci.yml — test"
if (( fast )); then
    cargo test --workspace -- --skip chaos_
    ok "tests (chaos skipped — run without --fast before pushing)"
else
    cargo test --workspace
    ok "tests"
fi

# ---------------------------------------------------------------------------
# .github/workflows/sparkplug-isolated.yml
#
# The one that went red for six commits. `--locked` is the whole point: it
# refuses to update the lock file, so an uncommitted dependency change fails
# here and nowhere else.
# ---------------------------------------------------------------------------
step "sparkplug-isolated.yml — isolated build (--locked)"
cargo build -p sparkplug-b --no-default-features --locked
ok "isolated build"

step "sparkplug-isolated.yml — context-leak guard"
cargo test -p sparkplug-b --test no_context_leak
ok "no_context_leak"

# ---------------------------------------------------------------------------
# cargo-deny runs as a GitHub action rather than a shell step, so it is only
# checked here when the tool happens to be installed.
# ---------------------------------------------------------------------------
step "deny.yml — licences and dependency direction"
if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny check
    ok "cargo-deny"
else
    echo "cargo-deny not installed — SKIPPED (CI will still run it)"
fi

# ---------------------------------------------------------------------------
# .github/workflows/docker-publish.yml — the image's own smoke tests.
#
# ADDED 2026-08-04, because this script did NOT cover this workflow and the gap
# cost a hung CI run: the image smoke test expected the binary to exit on an
# incomplete configuration, ADR 0023 made an absent configuration a first run
# that comes up and waits, and the step ran for 26 minutes against a 3 minute
# baseline before anyone looked. Every local run before that push was green,
# because this file reproduced two workflows out of three while its own header
# said it reproduced CI.
#
# Skipped under --fast: it builds a container.
# ---------------------------------------------------------------------------
if [[ $fast -eq 0 ]]; then
    step "docker-publish.yml — image build and smoke tests"
    docker build -t smartme_mqtt:ci . >/dev/null
    scripts/docker-smoke.sh
    ok "image smoke tests"
else
    step "docker-publish.yml — SKIPPED (--fast)"
    echo "the image is not built, so its smoke tests do not run."
    echo "anything that changes how the binary STARTS needs the full run."
fi

printf '\n\033[32m\033[1mAll CI steps reproduced locally.\033[0m\n'
