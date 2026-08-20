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

# THE HEALTHCHECK, and every number in it is derived rather than guessed
# ([ADR 0041], story 7.1).
#
# **Why it took until Epic 7.** The comment that stood here since Epic 0 said the
# `HEALTHCHECK` line "belongs in the same change as the endpoint it probes". That
# endpoint — `/healthz` with `last_loop_tick`, the wedge allowance and, since story
# 6.5, the sink's own state — arrived in Epic 6. And [#56] found what made this
# more than one line: this image has no `curl`, no `wget`, and a shell that is not
# bash, so nothing inside the container could consume `/healthz` at all. ADR 0041
# decides: the binary probes itself, which adds no package and cannot drift from
# the port the server binds.
#
# **What it fails on, and what it deliberately does not.** `/healthz` returns
# non-200 in exactly one state: a publishing bridge whose poll loop has not ticked
# in three poll periods. A restart repairs that. An unreachable broker, a refused
# credential and a degraded meter are all HEALTHY here — restarting repairs none of
# them and destroys every meter's Sparkplug session on the way past (ADR 0027 §2).
#
#   --interval=30s     Detection latency, not the wedge rule: `/healthz` computes
#                      the wedge itself against the observed cadence (AR12), so
#                      this only decides how soon a wedge already declared is
#                      noticed. 30 s is small against a 30 s poll period's
#                      three-period allowance.
#   --timeout=5s       The probe's own request timeout is 3 s; this is its ceiling,
#                      not a second deadline that could fire first.
#   --retries=3        ~90 s of continuous wedge before a restart. One missed
#                      answer during a reconfiguration must not kill a session.
#   --start-period=60s Startup only. A bridge with no configuration is silent BY
#                      DESIGN and answers 200, so this covers building the runtime
#                      and binding the port, nothing more.
HEALTHCHECK --interval=30s --timeout=5s --retries=3 --start-period=60s \
    CMD ["/usr/local/bin/smartme-bridge", "--healthcheck"]

ENTRYPOINT ["/usr/local/bin/smartme-bridge"]
