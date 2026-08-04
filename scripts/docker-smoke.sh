#!/usr/bin/env bash
# The image's smoke tests: a built image that cannot start is not a built image.
#
# ONE COPY, run by both the workflow and `ci-local.sh`. The workflow used to
# carry these inline, and `ci-local.sh` did not reproduce that workflow at all —
# so on 2026-08-04 a change to how the bridge starts left the "refuses to start"
# step HANGING in CI for 26 minutes against a 3 minute baseline, with every
# local run green. Two copies of a check drift; one copy cannot.
#
#   scripts/docker-smoke.sh [image-tag]     default: smartme_mqtt:ci
#
# Every step that starts the bridge carries its own deadline. That is not
# belt-and-braces: the failure above was a `docker run` with nothing to stop it,
# in a step with nothing to stop it either.

set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE="${1:-smartme_mqtt:ci}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() { echo "::error::$1"; exit 1; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }

# --- runs as non-root ------------------------------------------------------
id_line=$(timeout 60 docker run --rm --entrypoint id "$IMAGE")
echo "$id_line" | grep -q 'uid=10002' \
    || fail "image does not run as uid 10002"
ok "runs as uid 10002"

# --- a CA bundle is present ------------------------------------------------
timeout 60 docker run --rm --entrypoint ls "$IMAGE" \
    /etc/ssl/certs/ca-certificates.crt >/dev/null \
    || fail "no CA bundle — every HTTPS call to the smart-me cloud would fail, and the bridge would publish a permanent, honest STALE that looks exactly like a cloud outage"
ok "CA bundle present"

# --- refuses to start on an INVALID configuration --------------------------
#
# No default for group_id / node_id is a deliberate guard: a Sparkplug host
# PERSISTS what it discovers, so publishing into the wrong namespace is not
# undone by restarting with better settings.
#
# The identity is written EMPTY rather than omitted. Present-and-invalid is the
# case that refuses; absent is the case below, which serves the UI instead.
mkdir -p "$TMP/invalid"
cat > "$TMP/invalid/config.toml" <<'TOML'
schema_version = 3
group_id = ""
node_id = ""
broker_host = "broker.invalid"
broker_port = 1883
publish_period_secs = 30
mapping_confirmed = true

[[meters]]
meter_id = "m"
device_id = "d"
serial = "9202685"
enabled = true
TOML
out=$(timeout 60 docker run --rm \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/invalid:/state:ro" \
    "$IMAGE" 2>&1 || true)
echo "$out" | grep -q 'config.toml: group_id' \
    || { echo "$out"; fail "the image started without a group id; the guard is gone"; }
ok "refuses an invalid configuration, naming the key the operator edits"

# --- with NO configuration it comes up and STAYS up ------------------------
#
# The other half of the same seam. A bridge that exited here could never be
# configured at all: every setting but the credential arrives through the web
# UI, so the screen that writes the first config.toml sits behind this process.
mkdir -p "$TMP/empty"
out=$(timeout -s KILL 20 docker run --rm \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/empty:/state" \
    "$IMAGE" 2>&1; echo "EXIT:$?")
# PRESENCE first. Without this, "it did not exit" would also be true of an image
# wedged on something else entirely.
echo "$out" | grep -q 'no configuration yet' \
    || { echo "$out"; fail "an unconfigured image must SAY it is unconfigured, at a level visible under the DEFAULT filter"; }
# HAVING TO KILL IT IS THE PASS. GNU `timeout` exits 124 when it fires, even
# with `-s KILL`; 137 (128+SIGKILL) is what some other implementations report.
# Both mean the same thing here — the process was still running when the clock
# ran out — so both are accepted, and anything else is the process having chosen
# to exit.
#
# Written as 137 alone at first, which this very check caught on its first run.
echo "$out" | grep -qE 'EXIT:(124|137)' \
    || { echo "$out"; fail "the image exited without a configuration; the first run is then unreachable, since the screen that writes the first config.toml is served by this process"; }
[[ ! -f "$TMP/empty/config.toml" ]] \
    || fail "an unconfigured start wrote a config.toml; defaults nobody chose are still nobody's configuration"
ok "with no configuration: comes up, says so, stays up, writes nothing"

# --- a VALID but UNCONFIRMED mapping publishes nothing ----------------------
#
# The fourth startup state (Story 5.3). It shares "comes up and waits" with the
# state above and must NOT share its message: an operator who cannot tell "not
# configured" from "not confirmed" cannot act, and only the second is one click
# from fixed.
mkdir -p "$TMP/unconfirmed"
cat > "$TMP/unconfirmed/config.toml" <<'TOML'
schema_version = 3
group_id = "Plant"
node_id = "Bridge01"
broker_host = "broker.invalid"
broker_port = 1883
publish_period_secs = 30
mapping_confirmed = false

[[meters]]
meter_id = "garage"
device_id = "a1a1a1a1-b2b2-c3c3-d4d4-000000000001"
serial = "9202685"
enabled = true
TOML
out=$(timeout -s KILL 20 docker run --rm \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/unconfirmed:/state:ro" \
    "$IMAGE" 2>&1; echo "EXIT:$?")
echo "$out" | grep -q 'has NOT been confirmed' \
    || { echo "$out"; fail "a valid but unconfirmed mapping must say SO — and not in the same words as an absent configuration, which is a different problem with a different fix"; }
# Written as an explicit `if`, not `grep -q … && fail`. This is a NEGATIVE
# assertion, and under `set -e` an and-list whose first command fails is exactly
# the shape that makes an exit code describe something other than what was
# measured — a trap this repository has already paid for three times.
if echo "$out" | grep -q 'no configuration yet'; then
    echo "$out"
    fail "the unconfirmed state is reporting itself as UNCONFIGURED; the two must not share a message"
fi
echo "$out" | grep -qE 'EXIT:(124|137)' \
    || { echo "$out"; fail "the image exited on an unconfirmed mapping; it must stay up to serve the screen where the mapping is confirmed"; }
ok "with an unconfirmed mapping: stays up, and says something different"
