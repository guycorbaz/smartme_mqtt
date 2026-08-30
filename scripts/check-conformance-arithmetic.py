#!/usr/bin/env python3
"""Every number the conformance matrix states about itself must agree with its tables,
and every `gap (declined)` row must name the decision that declined it.

WHY THIS EXISTS. On 2026-08-04 six rows moved verdict. The tables were amended and
verified BY A SCRIPT — which checked the tables, i.e. the half of the document least
likely to be wrong. Every sentence that restated those numbers in prose survived:
`15 + 0 + 5 + 21`, `The 5 gaps`, `32 conformant is 31 distinct`, `The 43 gaps split
29 / 14`. That last one had already been corrected once, for the same reason.

The matrix deliberately keeps HISTORY ("the total was X until Story Y"), so a
restatement inside such a paragraph is a record of a past state and not a claim about
the present. The check is therefore per PARAGRAPH, not per line: an arithmetic
statement three lines below "until Story 4.10" belongs to that history.

THE DECLINED-ROW CHECK was added 2026-08-30 with ADR 0060, which made `declined` the
third label of the `gap` verdict. The label's entire content is "a decision exists":
a row wearing it without a link to that decision is the annotation-without-evidence
failure the ADR was written to end — thirteen rows had spent a month asserting a debt
in the Verdict column and a decision in the prose beside it.

FALSIFICATION. Both new checks are falsified on every run, not once by hand:
`--self-test` runs them against the fixtures in `fixtures/conformance/`, which carry
the ordinary shape of each fault — a declined row citing its ADR in bare prose, and a
split recomputed in one term and not the others — plus a fixture that must NOT be
flagged, so the guard proves discrimination rather than noise. See `CLAUDE.md`,
"Tests: falsify before trusting".
"""
import re
import sys
from pathlib import Path

HISTORY = re.compile(r"\bwas\b[^.]*\buntil\b|\buntil Story\b|Corrected", re.I)
# A verdict cell claiming `declined` must link the ADR, not merely mention one:
# `[ADR 0018](adr/0018-….md)`. A bare "ADR 0018" is what an author writes when
# copying the sentence rather than the citation.
ADR_LINK = re.compile(r"\]\((?:\.\./)*(?:docs/)?adr/\d{4}-[^)]*\)")


def check(s):
    """Return the list of disagreements found in the matrix text `s`."""
    problems = []

    tallies = [tuple(map(int, m.groups())) for m in
               re.finditer(r"\*\*(\d+) conformant · (\d+) deviations? · (\d+) gaps? · (\d+) n/a\*\*", s)]
    m_total = re.search(
        r"\| \*\*Total\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\*", s)
    if not m_total:
        return ["no whole-specification Total row found"]
    total = tuple(map(int, m_total.groups()))

    # The chapter rows of the whole-specification table must sum to the total row.
    rows = re.findall(r"^\| (\d+) — [^|]*\| *(\d+) \| *(\d+) \| *(\d+) \| *(\d+) \|", s, re.M)
    summed = [sum(int(r[i]) for r in rows) for i in range(1, 5)]
    if summed != list(total):
        problems.append(f"the chapter rows sum to {summed}, the Total row says {list(total)}")

    # EACH CHAPTER'S OWN TALLY MUST MATCH ITS ROW IN THE WHOLE-SPECIFICATION TABLE.
    #
    # Added 2026-08-19 (Story 4.19) because the document had disagreed with itself for a
    # day and this script had not noticed: Story 4.12 moved chapter 6 from `38 · 4 · 8`
    # to `39 · 4 · 7` in the chapter tally and left the summary row at `38 … 8`. The
    # check above still passed — the rows summed to a Total that was itself built from
    # the stale row, which is exactly how a consistent-looking document can be wrong.
    #
    # Tallies appear in chapter order, as do the rows, so they pair by position.
    if len(tallies) != len(rows):
        problems.append(f"{len(tallies)} chapter tallies against {len(rows)} rows in the total table")
    else:
        for (label, *cells), tally in zip([(r[0],) + r[1:] for r in rows], tallies):
            row_values = tuple(int(c) for c in cells)
            if row_values != tally:
                problems.append(
                    f"chapter {label}: its own tally says {list(tally)}, the whole-specification "
                    f"table says {list(row_values)}"
                )

    # EVERY `gap (declined)` ROW NAMES ITS ADR (ADR 0060, Decision 3).
    for line_no, line in enumerate(s.split("\n"), start=1):
        if not line.startswith("| `"):
            continue
        verdict = line.rsplit("|", 2)[1]
        if "gap (declined)" in verdict and not ADR_LINK.search(verdict):
            clause = line.split("|")[1].strip()
            problems.append(
                f"line {line_no}: {clause} is `gap (declined)` and links no ADR — the label asserts "
                f"a recorded decision, so the row must name it"
            )

    gap_counts = [total[2]] + [t[2] for t in tallies]
    offset = 0
    for para in s.split("\n\n"):
        line_no = s[:offset].count("\n") + 1
        offset += len(para) + 2
        if HISTORY.search(para):
            continue  # a record of a past state, not a claim about the present
        for m in re.finditer(r"`(\d+) \+ (\d+) \+ (\d+) \+ (\d+) = (\d+)`", para):
            v = tuple(map(int, m.groups()[:4]))
            if v != total and v not in tallies and sum(v) != 124:
                problems.append(f"line ~{line_no}: `{m.group(0)}` matches neither the total nor any chapter tally")
        for m in re.finditer(r"The (\d+) gaps", para):
            if int(m.group(1)) not in gap_counts:
                problems.append(f"line ~{line_no}: 'The {m.group(1)} gaps' — the total says {total[2]}")
        for m in re.finditer(r"\*\*(\d+) conformant is\s+(\d+) distinct\*\*", para):
            if int(m.group(1)) not in [t[0] for t in tallies]:
                problems.append(f"line ~{line_no}: '{m.group(1)} conformant is …' matches no chapter tally")
        for m in re.finditer(r"split (\d+) unimplemented / (\d+) unproven", para):
            if int(m.group(1)) + int(m.group(2)) != total[2]:
                problems.append(f"line ~{line_no}: split sums to {int(m.group(1))+int(m.group(2))}, the total says {total[2]}")
        # The three-label form (ADR 0060). It may state a chapter's split as well as the
        # whole document's, so it is checked against the same set as `The N gaps` — the
        # two-label form above predates that and is left alone rather than loosened.
        for m in re.finditer(r"split (\d+) declined / (\d+) unimplemented / (\d+) unproven", para):
            got = sum(int(g) for g in m.groups())
            if got not in gap_counts:
                problems.append(
                    f"line ~{line_no}: split sums to {got}, which is neither the total "
                    f"({total[2]}) nor any chapter's gap tally"
                )
    return problems


def self_test(fixtures):
    """Each fixture declares what this script must say about it, and is run to prove it."""
    failures = []
    paths = sorted(fixtures.glob("*.md"))
    if not paths:
        return [f"no fixtures found in {fixtures}"]
    for path in paths:
        text = path.read_text()
        m = re.search(r"<!-- expect: (clean|flagged) (.*?) -->", text)
        if not m:
            failures.append(f"{path.name}: no `<!-- expect: … -->` declaration")
            continue
        want, needle = m.group(1), m.group(2).strip()
        found = check(text)
        if want == "clean" and found:
            failures.append(f"{path.name}: must not be flagged, but was: {found}")
        elif want == "flagged" and not any(needle in p for p in found):
            failures.append(
                f"{path.name}: must be flagged with {needle!r}, got {found or 'nothing'}"
            )
    return failures


if __name__ == "__main__":
    here = Path(__file__).resolve().parent
    if "--self-test" in sys.argv:
        failures = self_test(here / "fixtures" / "conformance")
        if failures:
            print("the conformance checker fails its own falsification fixtures:")
            for f in failures:
                print(f"  {f}")
            sys.exit(1)
        print("conformance checker falsified against its fixtures: each fault is caught, "
              "the clean fixture is not flagged")
        sys.exit(0)

    doc = here.parent / "docs" / "sparkplug-conformance.md"
    problems = check(doc.read_text())
    if problems:
        print("conformance arithmetic disagrees with itself:")
        for p in problems:
            print(f"  {p}")
        sys.exit(1)
    text = doc.read_text()
    total = re.search(
        r"\| \*\*Total\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\*", text).groups()
    declined = sum(1 for line in text.split("\n")
                   if line.startswith("| `") and "gap (declined)" in line.rsplit("|", 2)[1])
    print(f"conformance arithmetic consistent: total {tuple(map(int, total))}, prose agrees; "
          f"{declined} declined rows, each naming its ADR")
