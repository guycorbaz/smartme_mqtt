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

Chapters are written incrementally alongside implementation. The smart-me authentication
section (Chapter 3) is complete; most operational chapters are stubs for now.
