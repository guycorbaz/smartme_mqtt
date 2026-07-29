# smartme_mqtt — Operator Manual (LaTeX)

A modern, KOMA-Script–based operator manual for `smartme_mqtt`, written in LaTeX.

## Build

Requires a LaTeX distribution (TeX Live) with `latexmk`. From this directory:

```sh
latexmk           # builds build/smartme_mqtt-manual.pdf via pdfLaTeX
latexmk -c        # clean auxiliary files
```

The `latexmkrc` pins pdfLaTeX and sends all artifacts to `build/` (gitignored).

## Layout

```
smartme_mqtt-manual.tex   main document (title page, includes chapters)
preamble/style.tex        packages, colours, fonts, callouts, code style
chapters/                 one .tex per chapter (filled in just-in-time per epic)
```

## Conventions

- **Callouts:** `\begin{note}…\end{note}`, `\begin{warning}…`, `\begin{tip}…`.
- **Code:** `lstlisting` (styled `modern`); inline `\code{...}`; the program name `\prog`.
- **Stubs:** `\stub{...}` marks a section to be written just-in-time as its epic lands.
- **Diagrams:** TikZ, with shared styles (`actor`, `brokeract`, `msg`, `msgdead`,
  `lifeline`, `mlbl`, `annot`) defined once in `preamble/style.tex` so every figure reads
  with one visual vocabulary. Message labels use `mlbl`, which is white-filled so it masks
  the lifeline it crosses.

## What describes the protocol, and what describes the product

Two chapters mention Sparkplug and they are deliberately different in kind. **Chapter 3,
*Understanding MQTT and Sparkplug B*, describes the specification** — it is background for
a reader who has not used MQTT, and it is the one place in this manual that documents
behaviour `smartme_mqtt` does not implement. **Chapter 6, *The MQTT/Sparkplug B contract*,
describes the product** and is the authority on what the bridge actually does.

Keep them apart when editing. Chapter 3 closes with a table mapping every mechanism onto
implemented / absent / deviates; that table is what stops the background chapter from
being read as a claim about the product, so amend it whenever the contract changes.

Chapters are written incrementally alongside implementation. The smart-me authentication
section (now Chapter 5, *Configuration*) is complete; most operational chapters are stubs
for now.

*Chapter files were renumbered when `02-understanding-sparkplug.tex` was inserted, so any
chapter number quoted outside this directory may be stale — check `\input` order in
`smartme_mqtt-manual.tex` rather than trusting a remembered number.*
