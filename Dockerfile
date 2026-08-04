# smartme_mqtt — single-arch image (Epic 7, AR11).
#
# Two stages: a builder that carries the Rust toolchain and the three tools this
# workspace's build actually needs, and a runtime that carries the binary, a CA
# bundle and nothing else.
#
# Build locally:
#   docker build -t smartme_mqtt .
# Published by `.github/workflows/docker-publish.yml` to docker.io/gcorbaz/smartme_mqtt.

# ---------------------------------------------------------------------------
# Builder
# ---------------------------------------------------------------------------
# The tag matches `rust-toolchain.toml` exactly. Pinning it here as well is
# deliberate duplication: the toolchain file would make the image *download* a
# second toolchain at build time if the two disagreed, which is slow and hides
# the mismatch. If you bump one, bump the other.
FROM rust:1.97.1-bookworm AS builder

# Three tools, each for a stated reason — this is not a copy of a template:
#   protobuf-compiler : `sparkplug-b` compiles the Tahu `.proto` at build time.
#   mold + clang      : `.cargo/config.toml` sets `linker = "clang"` and
#                       `-fuse-ld=mold` for x86_64-unknown-linux-gnu. Without
#                       them the build fails at the LINK step with a confusing
#                       "linker `clang` not found", long after compilation
#                       appears to have succeeded.
RUN apt-get update \
 && apt-get install -y --no-install-recommends protobuf-compiler mold clang \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# The whole workspace in one copy.
#
# No dependency-caching pre-step (the "copy manifests, build dummy main.rs"
# trick): this is a three-crate workspace where two crates ARE the dependencies
# of the third, so the trick would cache almost nothing while adding a layer
# that goes stale silently. CI caches the cargo registry instead, at the
# workflow level, where it can be invalidated by `Cargo.lock`.
COPY . .

# `--locked` is not optional. The `sparkplug-b isolated` workflow builds with it
# and stayed red for six commits while local builds were green, because
# `Cargo.lock` had not been committed. An image built from a resolved-fresh
# dependency graph is not the artifact CI tested.
RUN cargo build --release --locked -p smartme-bridge

# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# `ca-certificates` is REQUIRED, not hygiene. The bridge refuses a non-TLS
# `api_base` (`a_non_tls_api_base_refuses_to_start`) — a key in `config.toml`
# since 2026-08-04, the environment variable `SMARTME_API_BASE` before that — so every call to
# the smart-me cloud is HTTPS. Without a CA bundle the poll task fails on every
# tick and the bridge publishes an honest but permanent STALE — which looks
# exactly like a cloud outage and is the hardest possible way to discover a
# missing package.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Non-root, with a FIXED uid/gid so a bind-mounted state directory can be
# chowned to it from the host before first start.
#
# 10002 rather than 10001: `opcgw` on the same host already uses 10001, and two
# services sharing a uid means either can read and rewrite the other's
# bind-mounted state. The compose file carries the matching chown recipe.
RUN groupadd --gid 10002 smartme \
 && useradd --uid 10002 --gid 10002 --no-create-home --shell /usr/sbin/nologin smartme

# The persisted state directory. `SMARTME_STATE_DIR` defaults to `/data`, and
# TWO things live there:
#
#   config.toml  THE CONFIGURATION — every setting the bridge has, since
#                2026-08-04 (ADR 0023). Lose it and the bridge comes back
#                unconfigured, publishing nothing until somebody configures it
#                again. This comment described only `bdseq.toml` until a review
#                on 2026-08-05, which would have had anyone sizing a backup
#                around a session counter.
#   bdseq.toml   the Sparkplug session number. It MUST survive restarts: a bridge
#                that restarts with a fresh state directory replays a session
#                number, and a consumer that pairs a death to a birth by `bdSeq`
#                can then discard a death that belongs to a session it thinks is
#                still live.
#
# Mount a volume over this in compose.
RUN mkdir -p /data && chown 10002:10002 /data
VOLUME ["/data"]

COPY --from=builder /build/target/release/smartme-bridge /usr/local/bin/smartme-bridge

USER 10002:10002
WORKDIR /data

# NO HEALTHCHECK, and its absence is a decision rather than an omission.
#
# Architecture open item 7 is explicit: the healthcheck must reflect real poll
# state rather than mere process liveness, and **must not restart the container
# where an honest STALE is the better answer** — a restart destroys the very
# continuity the STALE is protecting. A process-liveness probe added now would
# do precisely the wrong thing: the bridge stays alive and correct through a
# cloud outage, so liveness is always true and the probe would be decoration;
# and any probe that DID fail on a stale reading would restart the container in
# the one case where restarting is harmful.
#
# The `/healthz` endpoint with the `last_loop_tick` heartbeat (AR12, FR33) is
# Epic 6. The HEALTHCHECK line belongs in the same change as the endpoint it
# probes, not before it.

ENTRYPOINT ["/usr/local/bin/smartme-bridge"]
