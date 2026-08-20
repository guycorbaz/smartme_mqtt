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

# --- refuses to PUBLISH on an INVALID configuration, and stays up ----------
#
# No default for group_id / node_id is a deliberate guard: a Sparkplug host
# PERSISTS what it discovers, so publishing into the wrong namespace is not
# undone by restarting with better settings.
#
# The identity is written EMPTY rather than omitted. Present-and-invalid is the
# case under test; absent is the case below.
#
# **Since ADR 0026 the container no longer exits here**, and this step had to
# change with it or it would have gone on passing while taking a minute to do
# it — `timeout 60` plus `|| true` would have swallowed the difference entirely.
# That is the drift this file's header is about, arriving a second time. So the
# check now uses the same shape as the two below: the fault must be named, AND
# the container must still be running when the clock runs out.
mkdir -p "$TMP/invalid"
cat > "$TMP/invalid/config.toml" <<'TOML'
schema_version = 4
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
# NAMED, and force-removed below. `timeout -s KILL` kills the `docker run`
# CLIENT; the container it started keeps running, `--rm` never fires, and the
# next step then shares the machine with a bridge nobody is watching. That was
# tolerable while this step's container exited on its own — it does not any more,
# so the step that made it stay up is the step that has to clean up after it.
inv="smartme_smoke_invalid_$$"
out=$(timeout -s KILL 20 docker run --rm --name "$inv" \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/invalid:/state:ro" \
    "$IMAGE" 2>&1; echo "EXIT:$?")
docker rm -f "$inv" >/dev/null 2>&1 || true
echo "$out" | grep -q 'config.toml: group_id' \
    || { echo "$out"; fail "the image published without a group id; the guard is gone"; }
echo "$out" | grep -q 'NOTHING is published' \
    || { echo "$out"; fail "the image must SAY it is withholding everything: now that it no longer exits to make the point, this line is the whole difference between a refused configuration and a working one"; }
# HAVING TO KILL IT IS THE PASS — see the note below on 124 vs 137.
echo "$out" | grep -qE 'EXIT:(124|137)' \
    || { echo "$out"; fail "the image exited on an invalid configuration; ADR 0026 keeps it up so the configuration screen can repair the file, and on a deployment with no shell that screen is the only repair there is"; }
ok "refuses to publish an invalid configuration, names the key, and stays up to be repaired"

# --- the HEALTHCHECK runs INSIDE the image, and reports healthy -------------
#
# **This is what [#56] said could not be done.** The image has no `curl`, no
# `wget` and a shell that is not bash, so nothing in it could consume `/healthz`
# — which meant AR12's restart could never fire in a real deployment. ADR 0041
# answers it: the binary probes itself, and this is the only place that claim can
# be checked against the actual image rather than against a unit test's idea of
# it (which is the defect [#56] recorded in the first place).
#
# **An unconfigured bridge must report HEALTHY.** It publishes nothing by design;
# a probe that called that unhealthy would put a first run into a restart loop
# and destroy the very screen the operator needs to configure it.
mkdir -p "$TMP/health"
hc="smartme_smoke_health_$$"
docker rm -f "$hc" >/dev/null 2>&1 || true
docker run -d --name "$hc" \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/health:/state" \
    "$IMAGE" >/dev/null
# PRESENCE FIRST: an image with no HEALTHCHECK reports `<no value>` here, and
# "not unhealthy" would be true of it too.
status=""
for _ in $(seq 1 60); do
    status=$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}NONE{{end}}' "$hc" 2>/dev/null || echo NONE)
    [[ "$status" == "starting" || "$status" == "NONE" ]] || break
    sleep 2
done
if [[ "$status" == "NONE" ]]; then
    docker rm -f "$hc" >/dev/null 2>&1 || true
    fail "the image declares no HEALTHCHECK: AR12's restart cannot fire in a deployment, which is [#56] unfixed"
fi
if [[ "$status" != "healthy" ]]; then
    docker inspect --format '{{json .State.Health}}' "$hc" || true
    docker rm -f "$hc" >/dev/null 2>&1 || true
    fail "an unconfigured bridge reported '$status': it publishes nothing BY DESIGN, and restarting it would destroy the screen that configures it"
fi
docker rm -f "$hc" >/dev/null 2>&1 || true
ok "the healthcheck runs inside the image, and a deliberately silent bridge is healthy"

# --- with NO configuration it comes up and STAYS up ------------------------
#
# The other half of the same seam. A bridge that exited here could never be
# configured at all: every setting but the credential arrives through the web
# UI, so the screen that writes the first config.toml sits behind this process.
mkdir -p "$TMP/empty"
# Named and force-removed, for the reason given at the invalid step above.
emp="smartme_smoke_empty_$$"
out=$(timeout -s KILL 20 docker run --rm --name "$emp" \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/empty:/state" \
    "$IMAGE" 2>&1; echo "EXIT:$?")
docker rm -f "$emp" >/dev/null 2>&1 || true
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
schema_version = 4
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
# Named and force-removed, for the reason given at the invalid step above.
unc="smartme_smoke_unconfirmed_$$"
out=$(timeout -s KILL 20 docker run --rm --name "$unc" \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/unconfirmed:/state:ro" \
    "$IMAGE" 2>&1; echo "EXIT:$?")
docker rm -f "$unc" >/dev/null 2>&1 || true
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

# --- the UI answers in a state where nothing is published -------------------
#
# Story 6.1: the states where the bridge is silent are exactly the states where
# the screen matters, because since ADR 0023 the browser is the only way in.
# An image that publishes nothing AND serves nothing cannot be configured at all.
# The port is published to LOOPBACK ON THE HOST for the duration of this check
# only. In production the container publishes nothing and Traefik reaches it over
# a shared Docker network (ADR 0019) — this is a probe, not the deployment shape.
#
# Probed from the host rather than from inside the container: `/dev/tcp` is a
# bash feature and the image's shell is not bash, which is how the first draft of
# this check reported "no web UI" against a server that was answering perfectly.
cid=$(docker run -d --rm \
    -p 127.0.0.1:18099:8080 \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/empty:/state" \
    "$IMAGE")
# shellcheck disable=SC2064
trap "docker rm -f $cid >/dev/null 2>&1 || true; rm -rf $TMP" EXIT
ui_ok=""
for _ in $(seq 1 30); do
    if answer=$(exec 3<>/dev/tcp/127.0.0.1/18099 2>/dev/null \
        && printf 'GET /healthz HTTP/1.0\r\n\r\n' >&3 && cat <&3); then
        if [[ "$answer" == *"200 OK"* ]]; then
            ui_ok=yes
            break
        fi
    fi
    sleep 1
done
docker rm -f "$cid" >/dev/null 2>&1 || true
trap 'rm -rf "$TMP"' EXIT
[[ -n "$ui_ok" ]] \
    || fail "the image serves no web UI while unconfigured — which is the one state that can only be left THROUGH the web UI"
ok "the UI answers on 8080 while the bridge is unconfigured"

# --- a first run, completed entirely over HTTP, against the real image -------
#
# Story 6.2 AC1 and AC7, and the only place they are checked against the thing
# that ships. The integration test drives the same journey against the binary;
# this drives it against the IMAGE, which is what a fresh deployment on panoramix
# actually runs — different filesystem ownership, different shell, a state
# directory that is a bind mount.
#
# It is also where NFR11 stops being an assertion. *"Time to first value under
# fifteen minutes from a clean machine"* has never been measurable, because until
# this story there was no way to reach a publishing bridge without editing a file
# by hand. The number is PRINTED, not merely bounded: a check that only compared
# against 900 s would stay green while the real figure crept from seconds to
# minutes, and the creep is the thing worth seeing.
PORT=18098
# Distinctive enough to search for: a secret of "x" is unsearchable, which is why
# the check below used to look for the variable's NAME instead.
SMOKE_SECRET="s3cr3t-smoke-do-not-print"
mkdir -p "$TMP/firstrun"
http() {  # method path [body]
    local method=$1 path=$2 body=${3-}
    exec 3<>/dev/tcp/127.0.0.1/$PORT || return 1
    if [[ -n "$body" ]]; then
        printf '%s %s HTTP/1.0\r\nHost: 127.0.0.1:%s\r\nOrigin: http://127.0.0.1:%s\r\n' \
            "$method" "$path" "$PORT" "$PORT" >&3
        printf 'Content-Type: application/x-www-form-urlencoded\r\nContent-Length: %s\r\n\r\n%s' \
            "${#body}" "$body" >&3
    else
        printf '%s %s HTTP/1.0\r\nHost: 127.0.0.1:%s\r\n\r\n' "$method" "$path" "$PORT" >&3
    fi
    cat <&3
    exec 3<&- 2>/dev/null || true
}

# An UNWRITABLE state directory first, because it is the failure a real
# deployment meets before it meets any other.
#
# The container runs as uid 10002 and the bind mount belongs to whoever created
# it — which is why `chown 10002:10002 ./data` is a documented deployment step
# and why forgetting it is the first thing that happens. Found by writing this
# check: the previous checks all READ the state directory, so nothing had ever
# tried to write one.
#
# The requirement is not that it succeed. It is that the screen say so. A form
# that reported "saved" over a write that failed would send an operator away
# believing the bridge is configured, and this whole project exists to not do
# that.
mkdir -p "$TMP/unwritable"
chmod 0555 "$TMP/unwritable"
cid=$(docker run -d --rm -p 127.0.0.1:$PORT:8080 \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET=x \
    -e SMARTME_STATE_DIR=/state -v "$TMP/unwritable:/state" "$IMAGE")
# shellcheck disable=SC2064
trap "docker rm -f $cid >/dev/null 2>&1 || true; chmod 0755 $TMP/unwritable 2>/dev/null; rm -rf $TMP" EXIT
for _ in $(seq 1 30); do
    [[ "$(http GET /config || true)" == *"200 OK"* ]] && break
    sleep 1
done
denied=$(http POST /config 'group_id=Plant&node_id=Bridge01&broker_host=192.0.2.1&broker_port=1883&publish_period_secs=30&meter.0.meter_id=garage&meter.0.device_id=d&meter.0.serial=9202685&meter.0.enabled=1' || true)
docker rm -f "$cid" >/dev/null 2>&1 || true
case "$denied" in
    *"200 OK"*)
        fail "a save that could not be written reported SUCCESS. On a deployment whose /data was never chowned to uid 10002 — the documented step everyone forgets — the operator would leave believing the bridge is configured" ;;
    *"NOT written"*) ;;
    *)
        echo "$denied"
        fail "a save onto an unwritable state directory must say so in the operator's own words" ;;
esac
ok "a save that cannot be written says so, and claims nothing"

# Now the journey, on a directory the container can actually write. 0777 rather
# than a chown: this script must run unprivileged, on a CI runner and on a
# laptop, and the uid inside the image is not one the host necessarily has.
chmod 0777 "$TMP/firstrun"

started=$(date +%s)
cid=$(docker run -d --rm \
    -p 127.0.0.1:$PORT:8080 \
    -e SMARTME_CLIENT_ID=x -e "SMARTME_CLIENT_SECRET=$SMOKE_SECRET" \
    -e SMARTME_STATE_DIR=/state \
    -v "$TMP/firstrun:/state" \
    "$IMAGE")
# shellcheck disable=SC2064
trap "docker rm -f $cid >/dev/null 2>&1 || true; rm -rf $TMP" EXIT

up=""
for _ in $(seq 1 30); do
    if [[ "$(http GET /config || true)" == *"200 OK"* ]]; then up=yes; break; fi
    sleep 1
done
[[ -n "$up" ]] || { docker logs "$cid" 2>&1 | tail -20; fail "the configuration screen never answered"; }

# TEST-NET-1 (RFC 5737) throughout: unroutable, so nothing can reach a broker or
# the smart-me cloud. What is under test is that the PHASE turns, not that a
# reading arrives.
form='group_id=Plant&node_id=Bridge01&broker_host=192.0.2.1&broker_port=1883'
form+='&publish_period_secs=30&api_base=https%3A%2F%2F192.0.2.1'
form+='&meter.0.meter_id=garage&meter.0.device_id=a1a1a1a1-b2b2-c3c3-d4d4-000000000001'
form+='&meter.0.serial=9202685&meter.0.enabled=1'
saved=$(http POST /config "$form" || true)
[[ "$saved" == *"200 OK"* ]] \
    || { echo "$saved"; docker logs "$cid" 2>&1 | tail -20; fail "the image refused a configuration the binary accepts — the difference is the image, and that is what this check exists for"; }
[[ -f "$TMP/firstrun/config.toml" ]] \
    || fail "the browser reported a save that never reached the bind mount; on a real deployment this is the /data ownership trap, and it must never pass silently"

preview=$(http GET /confirm || true)
[[ "$preview" == *"spBv1.0/Plant/DDATA/Bridge01/9202685"* ]] \
    || { echo "$preview"; fail "the confirmation screen must show the exact topic beside the serial"; }
mapping=$(printf '%s' "$preview" | grep -o 'name=mapping value="[0-9a-f]*"' | head -1 | sed 's/.*value="\(.*\)"/\1/')
[[ -n "$mapping" ]] || fail "the confirmation form carries no mapping fingerprint, so the click would bless whatever is on disk"

confirmed=$(http POST /confirm "mapping=$mapping" || true)
[[ "$confirmed" == *"303 See Other"* || "$confirmed" == *"200 OK"* ]] \
    || { echo "$confirmed"; fail "the confirmation was refused"; }

running=""
for _ in $(seq 1 40); do
    if [[ "$(http GET /healthz || true)" == *'"status":"running"'* ]]; then running=yes; break; fi
    sleep 1
done
elapsed=$(( $(date +%s) - started ))
docker logs "$cid" > "$TMP/firstrun.log" 2>&1 || true
docker rm -f "$cid" >/dev/null 2>&1 || true
trap 'rm -rf "$TMP"' EXIT

[[ -n "$running" ]] \
    || { tail -25 "$TMP/firstrun.log"; fail "AC7: confirming must start the publishing bridge with no human intervention. The container was still silent — which is the defect Story 6.2 was written to remove, and it survives only in the image"; }
# NFR12, and searched for the VALUE with a positive control.
#
# It grepped for the variable NAME until 2026-08-05, which cannot detect a leak
# at all — and it used the `&&`-list shape this same file spends four lines
# explaining is how three earlier checks went quiet. The secret is now a value
# distinctive enough to search for, and the search is proved able to find it
# before it is asked to prove it is absent.
if ! grep -q "$SMOKE_SECRET" <<<"leak:$SMOKE_SECRET"; then
    fail "the search itself is broken; this check proves nothing"
fi
if grep -q "$SMOKE_SECRET" "$TMP/firstrun.log"; then
    fail "the credential reached the container log"
fi
ok "a first run is completed entirely over HTTP against the image"

# --- FR40: A NEW CONTAINER ON THE SAME VOLUME IS AN UPDATE -------------------
#
# **What FR40's evidence was until this check, and why it was not enough.** FR40
# says "update by pulling a new image without config loss", and everything that
# tested it operated on the FILE: a struct written, read back, and compared. That
# is a claim about `serde`. Nobody had ever observed a container come up on a
# volume a previous container configured — which is the actual gesture, and the one
# where an ownership, a path or a schema mistake would show.
#
# The volume from the first-run block above is reused deliberately: it holds a
# configuration written by the image itself, confirmed through its own screens.
config_before=$(sha256sum "$TMP/firstrun/config.toml" | cut -d' ' -f1)
[[ -n "$config_before" ]] || fail "the first run left no configuration to update onto"

upd="smartme_smoke_update_$$"
docker rm -f "$upd" >/dev/null 2>&1 || true
docker run -d --name "$upd" \
    -e SMARTME_CLIENT_ID=x -e SMARTME_CLIENT_SECRET="$SMOKE_SECRET" \
    -e SMARTME_STATE_DIR=/state \
    -p "$PORT:8080" \
    -v "$TMP/firstrun:/state" \
    "$IMAGE" >/dev/null

republishing=""
for _ in $(seq 1 40); do
    if [[ "$(http GET /healthz || true)" == *'"status":"running"'* ]]; then republishing=yes; break; fi
    sleep 1
done
docker logs "$upd" > "$TMP/update.log" 2>&1 || true
config_after=$(sha256sum "$TMP/firstrun/config.toml" | cut -d' ' -f1)
docker rm -f "$upd" >/dev/null 2>&1 || true

[[ -n "$republishing" ]] \
    || { tail -25 "$TMP/update.log"; fail "FR40: a new container on an already-configured volume did not come up publishing. An update would then hand the operator a bridge asking to be configured again, with the configuration sitting right there"; }
[[ "$config_before" == "$config_after" ]] \
    || fail "FR40: the second container REWROTE the configuration ($config_before -> $config_after). A run that finds a file must not touch it: rewriting is how a schema stamp, a default or a dropped optional key silently edits what the operator wrote"
ok "an update — a new container on a configured volume — republishes and rewrites nothing"
printf '  \033[36mNFR11\033[0m time from a clean state directory to a publishing bridge: \033[1m%s s\033[0m\n' "$elapsed"
printf '        (budget is 900 s. This is the container only — it excludes pulling\n'
printf '         the image and the operator typing, which are the other two terms.)\n'
[[ "$elapsed" -lt 900 ]] || fail "NFR11: $elapsed s exceeds the fifteen-minute budget"
