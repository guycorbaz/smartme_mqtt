# Deferred Work

Items deferred from reviews; each carries its origin and where it should be picked up.

## Deferred from: code review of 1-2-domain-measurement-newtypes-quality (2026-07-25)

- String-key semantics before Epic 2 validation: `Serial::new("")` keys collide and `Eq`/`Hash` are case-sensitive (API vs config casing drift would create two logical devices) while Stories 1.9/1.10 already key maps on `Serial`/`MeterId`. Validation is scoped to Epic 2/5 per the story spec — revisit no later than Story 1.9.
- `TopicPath` accepts strings that are invalid as MQTT publish topics (empty string, `+`/`#` wildcards, interior NUL, leading/trailing `/`); failure surfaces only at the broker, far from the construction site. Well-formedness lands inside `new()` in Epic 2/5.
- Range policy unstated for physical/timestamp values: negative or `±inf` `Kwh` (a cumulative counter), pre-1970 `UtcMillis`, and the eventual `i64→u64` conversion at the Sparkplug encode boundary (wire timestamps are `uint64`). Epic 2 oracles / Story 1.8 own these guards.
