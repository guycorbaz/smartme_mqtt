<!-- expect: clean the control: a well-formed matrix, and nothing in it may be flagged -->

# Fixture — the control

This fixture exists to prove the guard DISCRIMINATES. It carries the shapes the guard is
allowed to see: a `gap (declined)` row that links its ADR, a `gap (unimplemented)` row that
links no ADR **and must not be flagged for it** (only `declined` asserts a decision), and a
three-label split that sums to the tally.

If this fixture ever starts failing, the guard has become noise.

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

The 2 gaps split 1 declined / 1 unimplemented / 0 unproven.
