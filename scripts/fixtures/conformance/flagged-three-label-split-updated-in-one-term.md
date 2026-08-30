<!-- expect: flagged split sums to -->

# Fixture — the ordinary shape of a stale split

A row moves to `declined`, the author increments the `declined` term, and does not decrement the
one it came from. The sentence then reads plausibly and counts one clause twice — the exact
failure the two-label form suffered twice before this check existed.

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `a-clause` | MUST | we do what it says | `a_test` | conformant |
| `b-clause` | MUST | we decline it, deliberately | — | **gap (declined)** — [ADR 0018](../../../docs/adr/0018-no-primary-host-state-the-repair-is-host-initiated.md) declines the mechanism |
| `c-clause` | MUST | not built yet, and owed | — | **gap (unimplemented)** ([#63](https://github.com/guycorbaz/smartme_mqtt/issues/63)) |

**1 conformant · 0 deviations · 2 gaps · 0 n/a**

| Chapter | conformant | deviations | gaps | n/a | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| 5 — Operational behaviour | 1 | 0 | 2 | 0 | 3 |
| **Total** | **1** | **0** | **2** | **0** | **3** |

The 2 gaps split 2 declined / 1 unimplemented / 0 unproven.
