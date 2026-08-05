# ADR 0024 — The configuration UI refuses submissions from other origins

- **Status:** accepted
- **Date:** 2026-08-05
- **Supersedes:** nothing. **Amends:** [ADR 0019](0019-no-auth-on-the-config-ui-secrets-are-write-only.md), by
  answering a question it left open.
- **Story:** 6.2 AC5 · **Issue:** [#54](https://github.com/guycorbaz/smartme_mqtt/issues/54)

## Context

[ADR 0019] put the trust boundary at Traefik and gave the bridge no login. That decision is
sound and this does not reopen it.

But story 6.2 turned the UI from a page that *reports* into a surface that *writes*, and
that exposed a consequence "no authentication" does not cover:

> **Precisely because there is no login, any page open in the operator's browser can post to
> the bridge if that browser can reach it.** A drive-by reconfiguration needs no credential
> to steal, because there are none.

The blast radius is not a defaced page. It is the Sparkplug namespace: a changed group or
node id creates a folder in a SCADA host's tag tree that outlives the process and is deleted
by hand, and a changed mapping publishes one meter's readings under another's identity —
which is the exact failure FR25's confirmation exists to prevent, arrived at from a
different direction.

Story 6.2 required this to be decided at drafting time rather than discovered later, because
a future reader will find a no-auth UI and reasonably assume nothing was considered.

## Decision

**Every mutating request is checked against the page it claims to come from, and refused
with `403` if it came from elsewhere. Nothing is written.**

The check, in `crates/smartme-bridge/src/ui/origin.rs`:

1. `Sec-Fetch-Site` is decisive where the browser sends it — anything but `same-origin` or
   `none` is refused.
2. Otherwise `Origin`'s authority is compared against `Host`.
3. **The scheme is deliberately not compared.** Traefik terminates TLS in front of a bridge
   that speaks plain HTTP on a Docker network, so `Origin` says `https://…` while the
   listener only ever sees `http`. Comparing schemes would refuse every legitimate
   submission on the only deployment this bridge has.
4. **A request with no `Origin` at all is allowed.** That is not a hole: browsers send
   `Origin` on every `POST`, so an attacker's browser sends one and it is wrong. What does
   *not* send it is `curl` — which is how FR23's headless bring-up and the container smoke
   test drive this surface. Refusing it would break the non-browser callers while stopping
   no browser attack.

## Alternatives considered

**A per-render token in a hidden field.** Stronger in principle, and rejected: a token needs
somewhere to be remembered between the render and the submission, and that somewhere is the
session [ADR 0019] deliberately does not have. Inventing one to guard a UI that has no login
is a large amount of machinery pointed at a smaller problem than the machinery itself.

**Nothing, with the reasons recorded.** Defensible while the UI only reported. It stopped
being defensible the moment a `POST` could change what reaches a SCADA host, and "the LAN is
trusted" is an assumption about the operator's browser rather than about the LAN — the
browser is the thing that carries the hostile page in.

## What this does NOT stop, and what actually stops it

*(Added 2026-08-05 after a fresh-context review. The original text read as though
the header check stood alone; it does not, and a reader who believed it would
mis-assess the deployment.)*

**A DNS rebinding attack defeats it, by construction.** `Origin`'s authority is
compared against `Host` — and both are supplied by the same request. That works
against an ordinary hostile page because the browser derives `Host` from the URL
the attacker's page used, and the attacker's own name is not the bridge's. It
does not work against an attacker who controls a name:

1. `attack.example` resolves, with a one-second TTL, to the attacker's server,
   which serves a page.
2. It re-resolves to the bridge's address.
3. The page posts to `http://attack.example:8080/config`.
4. The bridge sees `Origin: http://attack.example`, `Host: attack.example:8080`,
   `Sec-Fetch-Site: same-origin` — **and allows it**.

§4's claim that *"an attacker's browser sends one and it is wrong"* holds only
while the attacker cannot choose the name the browser used.

**What blocks it in the intended deployment is Traefik's `Host(...)` router
rule**, which drops a request whose `Host` is not the one it is configured for —
plus the fact that the container publishes no host port, so nothing outside the
shared Docker network can reach the listener at all. That is a property of the
reverse-proxy configuration, not of this code, and it belongs in Epic 7's
deployment artefacts as a requirement rather than as an accident.

A check against a *known expected host* would survive rebinding without Traefik.
It is not taken here because the bridge has no way to learn that name: it would
be a new configuration key, and therefore a schema bump, which is currently a
refusal-to-start on an older file. If the bridge is ever exposed by any route
other than a host-matching proxy, this is the first thing to revisit.

**Two smaller limits, recorded rather than fixed.** A request whose `Origin` is
not valid UTF-8 falls through the same `?` as an absent one and is allowed — not
browser-reachable, but the "absent means non-browser" reasoning silently covers
"unparseable" too. And `Sec-Fetch-Site: none` is accepted; browsers send it only
for user-initiated navigations, so it is unreachable for a `POST` and costs
nothing either way.

## Consequences

- The origin check adds **no configuration key and no schema bump**. That was a factor in
  choosing it: an allow-list of origins would have been a new field, and a new field is a
  schema version, which is currently a refusal-to-start on an older file.
- The refusal is a page, not a bare status: it names what was refused and says nothing was
  changed, because an operator who meets it deserves to know it was not their mistake.
- **This is not a substitute for authentication**, and must not be read as one. It stops a
  page in the operator's browser from acting on their behalf. It stops nothing that can
  reach the bridge directly — which remains Traefik's job and the reason the container
  publishes no host port.
- Tested in both directions. A guard that refused everything would pass a test that only
  checked refusals, and would make the UI unusable rather than safe.

## What would reopen this

Exposing the UI beyond the LAN, or giving it any action whose consequence is not confined to
this bridge's own namespace. Either makes the absence of authentication the larger problem
and this ADR the smaller one.

[ADR 0019]: 0019-no-auth-on-the-config-ui-secrets-are-write-only.md
