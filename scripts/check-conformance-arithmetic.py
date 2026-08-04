#!/usr/bin/env python3
"""Every number the conformance matrix states about itself must agree with its tables.

WHY THIS EXISTS. On 2026-08-04 six rows moved verdict. The tables were amended and
verified BY A SCRIPT — which checked the tables, i.e. the half of the document least
likely to be wrong. Every sentence that restated those numbers in prose survived:
`15 + 0 + 5 + 21`, `The 5 gaps`, `32 conformant is 31 distinct`, `The 43 gaps split
29 / 14`. That last one had already been corrected once, for the same reason.

The matrix deliberately keeps HISTORY ("the total was X until Story Y"), so a
restatement inside such a paragraph is a record of a past state and not a claim about
the present. The check is therefore per PARAGRAPH, not per line: an arithmetic
statement three lines below "until Story 4.10" belongs to that history.
"""
import re
import sys
from pathlib import Path

doc = Path(__file__).resolve().parent.parent / "docs" / "sparkplug-conformance.md"
s = doc.read_text()

tallies = [tuple(map(int, m.groups())) for m in
           re.finditer(r"\*\*(\d+) conformant · (\d+) deviations? · (\d+) gaps? · (\d+) n/a\*\*", s)]
total = tuple(map(int, re.search(
    r"\| \*\*Total\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\* \| \*\*(\d+)\*\*", s).groups()))

# The chapter rows of the whole-specification table must sum to the total row.
rows = re.findall(r"^\| (\d+) — [^|]*\| *(\d+) \| *(\d+) \| *(\d+) \| *(\d+) \|", s, re.M)
summed = [sum(int(r[i]) for r in rows) for i in range(1, 5)]
problems = []
if summed != list(total):
    problems.append(f"the chapter rows sum to {summed}, the Total row says {list(total)}")

HISTORY = re.compile(r"\bwas\b[^.]*\buntil\b|\buntil Story\b|Corrected", re.I)
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
        if int(m.group(1)) != total[2] and int(m.group(1)) not in [t[2] for t in tallies]:
            problems.append(f"line ~{line_no}: 'The {m.group(1)} gaps' — the total says {total[2]}")
    for m in re.finditer(r"\*\*(\d+) conformant is\s+(\d+) distinct\*\*", para):
        if int(m.group(1)) not in [t[0] for t in tallies]:
            problems.append(f"line ~{line_no}: '{m.group(1)} conformant is …' matches no chapter tally")
    for m in re.finditer(r"split (\d+) unimplemented / (\d+) unproven", para):
        if int(m.group(1)) + int(m.group(2)) != total[2]:
            problems.append(f"line ~{line_no}: split sums to {int(m.group(1))+int(m.group(2))}, the total says {total[2]}")

if problems:
    print("conformance arithmetic disagrees with itself:")
    for p in problems:
        print(f"  {p}")
    sys.exit(1)
print(f"conformance arithmetic consistent: total {total}, {len(tallies)} chapter tallies, prose agrees")
