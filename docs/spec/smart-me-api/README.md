# smart-me API — the description as published, kept here

**This directory is third-party content. It is NOT covered by this project's MIT licence, and its
own licence is unstated — see the caveat at the end.**

| | |
| --- | --- |
| Upstream | <https://api.smart-me.com/swagger/v1/swagger.json> |
| Human-readable form | <https://api.smart-me.com/swagger/index.html> |
| Declared version | **`v1`** (`info.version`), OpenAPI 3.0.4 |
| Retrieved | 2026-08-13T13:07:28Z |
| SHA-256 | `f6d21c58acab6d29d661148343a0dffbfdad889c6506c033daef571cae64dc57` |
| Size | 193 672 bytes, 64 paths, 6 405 lines |

`openapi-v1.json` is byte-for-byte what the endpoint served. It arrived already indented, so it is
diffable as it stands and nothing needed reformatting. **Do not tidy it** — the value of the copy is
that a refresh produces a diff that means something.

## Why a copy lives here

**A response does not say which version produced it.** There is no version segment in the path
(`/Devices/{id}`, never `/v1/Devices/{id}`) and no version header in the captured response
(`fixtures/http_headers/valid.txt` in `smart-me-client` shows the whole set: `date`, `content-type`,
`server`, `cf-cache-status`). The version exists only in this document's `info.version`. So the
bridge cannot detect that the API moved, and neither can anyone reading a log.

Keeping a copy converts that into something checkable: fetch the endpoint again, diff against this
file, and a change announces itself. That is the whole mechanism, and it costs one command:

```sh
curl -s https://api.smart-me.com/swagger/v1/swagger.json \
  | diff docs/spec/smart-me-api/openapi-v1.json - && echo "unchanged"
```

The same reasoning is why the Sparkplug specification has a copy at
`docs/spec/sparkplug-b-3.0.0/`: pinned beats current when the question is *what were we built
against*.

## What this document is worth, and what it is not

**It describes what the API declares. The capture in `crates/smart-me-client/fixtures/` records
what it actually sent.** The two are complementary and neither replaces the other.

**It is wrong about property names, and that matters.** The schema declares camelCase
(`activePower`, `valueDate`); the wire sends PascalCase (`ActivePower`, `ValueDate`), which the
captured fixture proves. This is the usual behaviour of .NET schema generators — the serializer and
the generator disagree. **Read this document for presence, type and nullability. Never for
casing.** `Device` in `crates/smart-me-client/src/types.rs` keeps `#[serde(rename_all =
"PascalCase")]` for that reason.

## The finding that came with it (2026-08-13)

Comparing the declared `Device` schema against the eight fields the client consumes:

| field | declared | nullable |
| --- | --- | --- |
| `Id`, `Serial` | yes | no |
| `Name`, `ActivePower`, `ActivePowerUnit`, `CounterReading`, `CounterReadingUnit`, `ValueDate` | yes | **yes** |

**Six of the eight are declared nullable and the deserializer requires all eight.** Measured rather
than assumed:

```
"ActivePower": null   =>  invalid type: null, expected f64 at line 3 column 31
"ActivePower" absent  =>  missing field `ActivePower` at line 2 column 76
```

Two consequences, both recorded and neither settled here:

- **An explicit `null` loses the field name.** A missing field keeps it; a null gives a line and a
  column into a payload no operator ever sees. That is story 2.6 AC5 — [#73] — and the API declares
  the nameless path possible on six of our eight fields.
- **A null in one metric would cost the whole reading**, including a cumulative energy index read
  and converted perfectly. That is the failure story 2.5 repaired for units, arriving through the
  schema instead. ADR 0031 says a verdict belongs to a metric; a null does not respect that yet.

Also learned from the paths, and useful elsewhere: `GET /Devices` exists, which is the data source
story 3.4's drop-down needs; and `GET /DeviceBySerial` exists, which may bear on ADR 0029's
after-the-fact serial check.

## Refreshing it

Re-fetch, replace the file, update the retrieval date and the SHA-256 above, and **read the diff** —
that is the point of the exercise, not a formality. A change to a field this bridge consumes is an
audit task: the fixture in `crates/smart-me-client/fixtures/` and the `Device` type are what a
change lands on.

## Licence caveat, stated rather than assumed

The document is served publicly and without authentication, and carries no licence statement of its
own. This copy is kept for reference and comparison, unmodified and attributed. **That is not the
same as knowing it is redistributable**, and nobody here has established that it is. If this
repository is ever published, that question needs an answer rather than an assumption — unlike
`docs/spec/sparkplug-b-3.0.0/`, whose EPL-2.0 terms are explicit and included.

`cargo-deny` does not govern this: it audits code dependencies, and this is a document.
