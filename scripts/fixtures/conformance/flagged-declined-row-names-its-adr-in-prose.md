<!-- expect: flagged links no ADR -->

# Fixture — the ordinary shape of a declined row with no citation

The fault is not exotic. An author moving a row to `declined` writes the sentence they have in
their head — *"ADR 0018 declines the mechanism"* — and does not make it a link, because the
sentence reads as if it already carried the evidence. That is how thirteen rows came to assert a
decision in prose while their Verdict column asserted a debt.

| tck-id | Level | Our behaviour | Proof | Verdict |
| --- | --- | --- | --- | --- |
| `a-clause` | MUST | we do what it says | `a_test` | conformant |
| `b-clause` | MUST | we decline it, deliberately | — | **gap (declined)** — ADR 0018 declines the mechanism |

**1 conformant · 0 deviations · 1 gap · 0 n/a**

| Chapter | conformant | deviations | gaps | n/a | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| 5 — Operational behaviour | 1 | 0 | 1 | 0 | 2 |
| **Total** | **1** | **0** | **1** | **0** | **2** |
