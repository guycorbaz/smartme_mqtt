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

# ---------------------------------------------------------------------------
# Every run leaves a line behind ([#94] follow-up, R6)
# ---------------------------------------------------------------------------
#
# The register has carried R6 — *the gate refuses one push in two, and the
# obvious workaround is to bypass it* — since 2026-08-19, rated on nothing but
# recollection. Two causes were found and repaired on 2026-08-23; whether that
# ended it can only be answered by counting, and until now this script recorded
# NOTHING: not a date, not a duration, not which step went red.
#
# That is not a detail of bookkeeping. The 2026-08-22 follow-up lowered its
# guard on "three days, twenty-six commits, no occurrence" and two refusals
# landed the same evening. A memory of calm is not a measurement of calm.
#
# The log lives in `.git/`: it is per-clone, never committed, survives
# `cargo clean`, and describes THIS machine — which is the only thing it can
# honestly describe. Reading it: `column -t .git/ci-local-runs.tsv`.
RUN_LOG="$(git rev-parse --git-dir)/ci-local-runs.tsv"
RUN_STARTED=$SECONDS
CURRENT_STEP="startup"
FAILED_TESTS=""

# Written by an EXIT trap rather than at the end, because `set -e` means a
# failing run never reaches the end — and a log that only records successes
# would answer the opposite of the question being asked.
log_run() {
    local status=$?
    local verdict="pass"
    (( status == 0 )) || verdict="fail"
    local mode="full"
    (( fast )) && mode="fast"
    printf '%s\t%s\t%ss\t%s\t%s\t%s\t%s\n' \
        "$(date -Iseconds)" \
        "$verdict" \
        "$(( SECONDS - RUN_STARTED ))" \
        "$mode" \
        "$(git rev-parse --short HEAD 2>/dev/null || echo '?')" \
        "${CURRENT_STEP}" \
        "${FAILED_TESTS:--}" \
        >>"$RUN_LOG"
}
trap log_run EXIT

step() { CURRENT_STEP="$1"; printf '\n\033[1m── %s\033[0m\n' "$1"; }
ok()   { printf '\033[32m✓ %s\033[0m\n' "$1"; }

# A test that has flaked this many times stops being tolerated.
#
# THIS THRESHOLD IS A POLICY, not a measurement, and it is the one number here
# worth arguing about. Three says: once is noise, twice is bad luck, three times
# is a defect nobody has looked at. Lower it and the gate nags; raise it and the
# quarantine becomes the place flakes go to be forgotten, which is the failure
# this whole mechanism exists to avoid.
FLAKE_BUDGET=3
QUARANTINE="$(git rev-parse --git-dir)/ci-local-quarantine.tsv"

# Runs `cargo test …`, and on failure runs it ONE more time before refusing.
#
# # Why a retry, when a gate exists to refuse
#
# R6 is not "the gate refuses" — it is "the workaround is to bypass it". A gate
# that refuses a push for a reason unrelated to the change is what makes
# `--no-verify` reasonable, and `CLAUDE.md` treats that flag as an on-record
# claim. One retry removes the temptation for the case where it is illegitimate,
# and for no other case.
#
# # The retry repeats the STEP, never the test alone
#
# Re-running just the failed test would be cheaper and would be wrong. [#94] was
# red in the full suite and green in isolation — 42 targeted runs found nothing —
# because the defect needed another thread to reach a callsite first. A retry
# that isolated the test would have called it a flake and hidden a real defect
# for as long as it took someone to disbelieve the gate. Same command, same
# conditions, or the second run answers a different question than the first.
#
# # What is bought with the second run is recorded, not spent
#
# A pass on the retry lets the push through AND writes the names to the
# quarantine file. Nothing is silently forgiven: the count is printed on every
# subsequent run, and at `FLAKE_BUDGET` the gate refuses. That is what keeps this
# from becoming an amnesty — the tolerance is a countdown, not a pardon.
tested() {
    local out
    out="$(mktemp)"
    if "$@" 2>&1 | tee "$out"; then
        rm -f "$out"
        return 0
    fi
    # `cargo test` lists them under `failures:`, one indented name per line, and
    # prints that block twice — hence `sort -u`.
    FAILED_TESTS="$(awk '/^failures:$/{f=1;next} /^test result:/{f=0} f&&NF==1{print $1}' "$out" \
        | sort -u | paste -sd, -)"
    rm -f "$out"

    # Spent its budget already? Then it is not a flake any more, and saying so is
    # the whole point of counting.
    local spent name
    for name in ${FAILED_TESTS//,/ }; do
        spent=$(grep -cF "	${name}	" "$QUARANTINE" 2>/dev/null || true)
        if (( spent >= FLAKE_BUDGET )); then
            printf '\n\033[31m\033[1m✗ %s has now failed-then-passed %s times.\033[0m\n' \
                "$name" "$spent"
            echo "That is no longer a flake, and this gate will not retry it again."
            echo "Its history: grep '$name' $QUARANTINE"
            return 1
        fi
    done

    printf '\n\033[33m── retrying the step once (R6): %s\033[0m\n' "$FAILED_TESTS"
    echo "A second run of the SAME command, not of the failed test alone: [#94] was"
    echo "red in the suite and green in isolation, so isolating would ask a different"
    echo "question than the one that failed."
    if ! "$@"; then
        echo "failed twice — this is not a flake."
        return 1
    fi

    # Passed on the retry. Recorded, printed, and counted against the budget.
    for name in ${FAILED_TESTS//,/ }; do
        printf '%s\t%s\t%s\t%s\n' \
            "$(date -Iseconds)" "$name" "$(git rev-parse --short HEAD 2>/dev/null || echo '?')" \
            "$CURRENT_STEP" >>"$QUARANTINE"
        spent=$(grep -cF "	${name}	" "$QUARANTINE" 2>/dev/null || true)
        printf '\033[33m⚠ %s failed then passed — %s of %s before this gate refuses it\033[0m\n' \
            "$name" "$spent" "$FLAKE_BUDGET"
    done
    FAILED_TESTS=""
    return 0
}

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
# The reference deployment parses — both ways it is offered (story 7.2, FR39)
# ---------------------------------------------------------------------------
#
# `docker-compose.yml` IS the deployment for this project: the manual tells the
# operator to copy it. It carries a commented `ports:` block for deployments
# without Traefik, and a commented block is a block nothing has ever parsed —
# which is how a file comes to offer a fallback that does not work.
#
# This validates the file as shipped, and again with the fallback uncommented.
# It proves the shape, not that Traefik routes: standing up Traefik in a gate
# would be proving somebody else's software.
step "docker-compose.yml — the reference deployment parses, both ways"
if ! command -v docker >/dev/null 2>&1; then
    # SAID, not skipped in silence. A check that vanishes when a tool is absent
    # reads afterwards exactly like a check that passed.
    printf '\033[33m~ skipped: docker is not on this machine\033[0m\n'
else
    # The credential variables use Compose's `:?` form, which fails immediately
    # when unset — deliberately, so a deployment cannot start without them. The
    # gate supplies throwaway values.
    #
    # **On the command, NEVER exported** — repaired by the review of story 7.2. An
    # `export` here would have left `SMARTME_CLIENT_ID` and `SMARTME_CLIENT_SECRET`
    # set for every step BELOW, including the whole Rust suite. This repository has
    # already recorded what that costs: story 6.6's own test had to be made immune
    # to those variables, because other tests in the same binary set them and the
    # environment is per-process. A gate that quietly hands the suite a credential
    # is a gate that changes what it is measuring.
    if ! SMARTME_CLIENT_ID=gate SMARTME_CLIENT_SECRET=gate \
        docker compose -f docker-compose.yml config -q; then
        echo "docker-compose.yml does not parse; the manual tells the operator to copy it"
        exit 1
    fi
    # IN THE REPOSITORY, not in /tmp: the file says `env_file: .env`, and Compose
    # resolves that relative to the file's own directory. A copy in /tmp looks for
    # /tmp/.env and fails for a reason that has nothing to do with what is tested.
    fallback="$(mktemp ./.compose-fallback.XXXXXX.yml)"
    # Uncomment the `ports:` block: the two lines are `#ports:` and its mapping.
    sed 's/^\( *\)#ports:/\1ports:/; s/^\( *\)#  - "\(.*\)"/\1  - "\2"/' \
        docker-compose.yml > "$fallback"
    if ! SMARTME_CLIENT_ID=gate SMARTME_CLIENT_SECRET=gate \
        docker compose -f "$fallback" config -q; then
        rm -f "$fallback"
        echo "the non-Traefik fallback does not parse — a deployment without Traefik is documented and would fail at the first command"
        exit 1
    fi
    # PRESENCE BEFORE VERDICT: if the sed matched nothing, both files are the
    # same and the check above proved nothing about the fallback.
    if ! grep -qE '^\s*ports:' "$fallback"; then
        rm -f "$fallback"
        echo "the fallback was never uncommented, so this check proved nothing — the commented block's shape must have changed"
        exit 1
    fi
    rm -f "$fallback"
    ok "the reference deployment parses, with and without the Traefik fallback"
fi

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
    # Skipped by NAME, so every Docker-dependent test must carry one of these.
    #
    # `unconfirmed_publishes_nothing.rs` (Story 5.3) needs testcontainers and is
    # named after its property rather than after its harness, so `--fast` stopped
    # skipping what it promises: on a machine without Docker it panicked at
    # "broker container starts" instead of being skipped, and the documented
    # no-Docker path was broken by a story that never touched this file.
    tested cargo test --workspace -- --skip chaos_ \
        --skip a_confirmed_mapping_does_birth \
        --skip an_unconfirmed_mapping_never_reaches \
        --skip an_unconfigured_bridge_never_reaches
    ok "tests (broker-dependent tests skipped — run without --fast before pushing)"
else
    tested cargo test --workspace
    ok "tests"
fi

# The one test that needs a Cargo feature, and it is copied from ci.yml like
# everything else here.
#
# Story 6.1 AC5's panic half ([#51]) could not be asserted without a route that
# panics, and shipping one was refused. `panic-probe` adds exactly that route
# and nothing else; `docker-publish.yml` builds with default features, so no
# released image carries it. Run separately because `--workspace` does not
# enable it — which also means that WITHOUT this step the feature would be dead
# code and the guard untested, the failure mode the step exists to prevent.
step "ci.yml — the panicking-handler guard (feature-gated route)"
tested cargo test -p smartme-bridge --features panic-probe \
    --test a_panicking_handler_does_not_cost_the_meters
ok "a panicking handler costs the page and nothing else"

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
# Not a CI step either, and added 2026-08-04 after a review found the same
# defect for the SECOND time: the conformance matrix states its own numbers in
# prose as well as in tables, and an amendment that fixes the tables leaves the
# prose saying the old ones. A script had already been run that day — it checked
# the tables.
# ---------------------------------------------------------------------------
step "the conformance matrix agrees with itself"
python3 scripts/check-conformance-arithmetic.py
ok "conformance arithmetic"

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
