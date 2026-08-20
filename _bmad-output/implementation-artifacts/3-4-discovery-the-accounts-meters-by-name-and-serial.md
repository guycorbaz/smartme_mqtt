# Story 3.4: Discovery — the account's meters, by name and serial

Status: done (2026-08-15) — written, implemented, reviewed (three angles, six repairs) and the
repair reviewed in its own right (four more, one caught by GitHub's AC6 word-scan after the
repair skipped the full gate — the process lapse is named in `b538835`). Two rounds, the second
smaller than the first; all three workflows green on the final push

## Story

As the operator configuring the bridge,
I want the configuration screen to offer me the meters my smart-me account actually has,
so that adding a meter is picking it rather than transcribing two identifiers — and so that the
most likely configuration error there is (a mistyped device id, story 2.6's `404` polled for
ever) becomes hard to produce instead of merely loudly refused.

## Why this exists, and the decision that shaped it

FR2 (*discover the account's meters, identified by name and serial*) was deliberately NOT a
prerequisite for the fleet — the mapping is typed or pasted today and that works. Two things
changed its standing:

1. **The deferral acquired an observed cost** (2026-08-13). Story 2.6's review found an unknown
   device id published as a network fault and polled for ever. Guy's reading: *"quand on ajoute
   des compteurs à smartme_mqtt, ils proviennent de ce qui existe chez smart-me : donc un
   compteur inexistant ne devrait pas apparaître sauf faute de frappe."* A pick-list is what
   makes the fault rare; ADR 0029 and the latching refusal are what keep it honest when it
   happens anyway.
2. **The data source stopped being a code comment** (2026-08-13). `GET /Devices` is now readable
   in the pinned description (`docs/spec/smart-me-api/openapi-v1.json`): an array of `Device`,
   declaring — as everywhere — only `200`. For discovery, three fields matter: `id` (uuid, not
   nullable), `serial` (int64, not nullable), `name` (string, **nullable**).

**Prevention and honesty are both required, and honesty comes first** (decided 2026-08-13,
recorded in the epic): the file is editable by hand, a picked device can disappear afterwards,
and the list is one instant while the fetch is every period. Nothing in this story may soften a
refusal because "the id came from the list".

## Decisions taken at drafting (no later story may re-choose them)

1. **The listing type is NOT `Device`.** `Device` requires all eight fields it consumes and the
   API declares six of them nullable ([#74]) — reusing it would silently eject from the list any
   real meter whose momentary reading carries a null, which is a meter the operator cannot pick
   for a reason nobody is told. Discovery deserializes its own thin type: `id`, `serial`,
   `name` (optional — a null name displays the serial; nothing invents a name).
2. **Discovery is ON DEMAND, from the screen.** A periodic discovery loop is story 3.5's subject
   (FR6 compares the list over time to see a meter disappear); building it here would decide
   3.5's mechanism as a side effect. One fetch per explicit operator request, no cache — the
   staleness of a cached list is exactly the "one instant" hazard the epic names.
3. **The 5.3 boundary is drawn as: reading smart-me is not publishing.** "Nothing is published
   until the mapping is confirmed" (story 5.3) constrains the WIRE; discovery is a read of the
   source with the stored credential and changes nothing — no publish, no adoption, no config
   write. The screen may therefore discover before, during and after confirmation alike.
4. **A partial list is shown WITH its omission said, never silently.** For measurements this
   repository is fail-closed; a LISTING is the opposite trade: an element that fails to parse
   costs that element (counted and named on the screen), not the whole list — because an empty
   screen with no explanation is the silent failure, and a missing meter with a stated reason is
   absence published as absence.
5. **[#64] is settled here, before any field is added.** The repository's remedy for
   two-readers-one-question is the exhaustive destructure that stops the build until somebody
   answers (`reconfigure::classify`'s pattern). What belongs to the mapping is decided field by
   field, in one place both `store::same_mapping` and `ui::screens::mapping_fingerprint` read
   from — landing BEFORE this story's temptation to add a field is the point of its deadline.

## Acceptance Criteria

**AC1 — The client lists the account's devices, and one bad element does not cost the list.**

**Given** a `GET /Devices` body containing well-formed devices and one malformed element
**When** it is decoded
**Then** the well-formed devices are returned AND the malformed one is reported — count and
serde reason, the field-naming discipline of story 2.6 AC5 — never silently dropped
**And** the decode is a pure function (`decode_devices`, beside `decode_device`), per action C1:
this property must be assertable without an HTTP harness
**And** a body that is not a JSON array at all fails as a whole, with serde's reason kept
**And** an empty array is a successful, empty listing — a state, not a fault.

**AC2 — Picking a meter fills the PAIR, on the screen the operator already uses.**

**Given** a reachable account with meters
**When** the operator asks the configuration screen for them
**Then** the screen offers the account's meters — name when present, serial always, serial alone
when the name is null (nothing invents a name)
**And** choosing one fills `device_id` AND `serial` together — the pair ADR 0029 binds at
runtime becomes one gesture instead of two transcriptions
**And** typed entry remains possible and unchanged: the file is editable by hand, and discovery
being down must not lock the form.

**AC3 — Discovery failures use the error taxonomy and reach the operator.**

**Given** the discovery call fails
**When** the screen renders
**Then** a `401/403` names the credential, a timeout/transient names the source's
reachability, and each failure names its repair — rendered on the page (action C4: text written
for the operator has a test that it renders), with the form still usable for typed entry
**And** an empty account renders as the state it is ("no meters on this account"), not as a
fault.

**AC4 — Discovery is read-only and confirmation-independent.**

**Given** an unconfirmed mapping (story 5.3's state)
**When** discovery is requested
**Then** it answers — reading the source is not publishing — and it causes no publish, no
memory adoption, and no configuration write of its own
**And** a test drives discovery in the unconfirmed state and asserts nothing reaches the outbox.

**AC5 — [#64] closes: the mapping's membership is decided in one place, mechanically.**

**Given** `StoredConfig` and `StoredMeter`
**When** any field is added to either
**Then** the build stops until the new field is classified as mapping or not-mapping (exhaustive
destructure), and `same_mapping` and `mapping_fingerprint` both read the classification —
two readers, one answer, by construction
**And** the classification of every existing field is recorded with its reason
**And** [#64] is closed in the same commit, citing the mechanism.

**AC6 — The pick-list softens nothing.**

**Given** a configured device id that the account does not have — picked from a stale list,
typed, or hand-edited into the file
**When** the bridge fetches it
**Then** the refusal is exactly story 2.6's: `Fatal`, latching, `configuration-contradicted`
naming the id — asserted by a test that exists to keep "it came from the list" from ever
becoming an excuse (the honesty-first decision of 2026-08-13, pinned).

**AC7 — Falsified before trusted, and RUN before recorded** (action C3: a falsification note
without the run's copied output is a prediction).

**AC8 — `./scripts/ci-local.sh` full run green, then `gh run list`.**

## Tasks / Subtasks

- [x] **Task 1 — The listing decode** (AC1) — 2026-08-15
  - [x] `DeviceListing` in `smart-me-client` (`id`, `serial`, optional `name`) — deliberately
        not `Device`, the [#74] reason in its doc; an OMITTED `Name` reads as a null one
  - [x] `decode_devices`: pure, tolerant per element, drops counted and named, non-array fails
        whole, empty array is a listing (`DeviceList { devices, dropped }` — the type obliges
        the caller to see the drops)
- [x] **Task 2 — `GET /Devices` on the client** (AC1, AC3) — 2026-08-15
  - [x] `get_devices()` beside `get_device()`, same auth dance, no `Date` capture (a listing
        feeds a screen, not an oracle). **The collection `404` decided**: intercepted BEFORE
        `classify_device_status`, because that function's `404` is a fatal unknown-DEVICE
        refusal naming an id — a diagnosis with no subject here. It maps to the visible,
        transient `HttpStatus`, recorded in the method doc
- [x] **Task 3 — The screen offers the account** (AC2, AC3, AC4) — 2026-08-15
  - [x] `POST /config/discover`, origin-guarded (ADR 0024); reached from INSIDE the main form
        via `formaction`, so unsaved edits ride along and come back in the boxes
  - [x] Pick fills `device_id` + `serial` together (the button's value carries the pair;
        no refetch — the pair is verified where it always was, by ADR 0029); typed entry
        untouched; `enabled` stays a deliberate tick, never a side effect of picking
  - [x] Taxonomy + empty state + partial-list caveats rendered by the PURE
        `discovery_section(Option<&Discovery>)` (C1), asserted on bytes
- [x] **Task 4 — The unconfirmed-state proof** (AC4) — 2026-08-15: the pick test runs against
      an Unconfigured phase and a store with no file, and asserts `!store::exists` after
- [x] **Task 5 — The mapping-membership mechanism** (AC5) — 2026-08-15, and this story adds
      NO stored field, which is the cheapest proof the guard was worth landing first:
      `store::mapping_projection` (exhaustive destructure, per-field classification with
      reasons), read by both `same_mapping` and `mapping_fingerprint`. [#64] closes with the
      commit
- [x] **Task 6 — The refusal stays** (AC6) — 2026-08-15: anchored on
      `an_unknown_device_id_latches_instead_of_being_retried_for_ever`, whose doc now names
      the excuse this story creates ("it came from the list") and why it never softens the
      latch. No new code path from the pick-list to `map_error` exists to test — the pick
      writes form values, and everything after Save is the machinery already pinned
- [x] **Task 7 — Falsify** (AC7) — 2026-08-15, seven mutations, table in the notes; every one
      run before its note
- [x] **Task 8 — `./scripts/ci-local.sh` full run**, then `gh run list` — 2026-08-15, EXIT=0
      end to end (chaos and image included; the port-8080 impediment is lifted, so a local
      failure would have been a real one)

## Dev Notes

### The traps this story is most likely to fall into

1. **Reusing `Device` for the listing.** The account's fourth meter (`exterieur`, unplugged,
   ValueDate four months old) is exactly the shape that could carry nulls — and it is a meter
   the operator must be able to SEE to decide about. The thin type is not an optimization; it is
   what keeps discovery honest about meters whose readings are currently unreadable.
2. **Letting the pick-list argue against the refusal.** AC6 exists because "the id came from the
   list" will be true and irrelevant: the list was one instant. ADR 0029's latch and story 2.6's
   `configuration-contradicted` stay exactly as they are.
3. **Settling [#64] by derive.** A mechanical fold of every field into the fingerprint would
   withdraw confirmations on changes that have nothing to do with the mapping — the same defect
   wearing the fix. The destructure must DECIDE per field, and the decision text is part of the
   deliverable.
4. **A discovery loop.** FR6 (3.5) owns compare-over-time. This story's discovery has no state
   between requests.

### Where the code lives

- `crates/smart-me-client/src/client.rs` — `get_device`, `decode_device` (the pure-extraction
  precedent, C1), `classify_device_status`; `get_devices` goes beside them
- `crates/smart-me-client/src/types.rs` — `Device` and the [#74] exposure the listing type must
  not inherit
- `crates/smartme-bridge/src/ui/screens.rs` — the configuration form, `mapping_fingerprint`
  (four fields by name — AC5's second reader), `confirm_mapping`, the `body()` test helper
- `crates/smartme-bridge/src/app/store.rs` — `StoredConfig`/`StoredMeter`, `same_mapping`
  (derived `==` — AC5's first reader)
- `crates/smartme-bridge/src/adapters/smartme_source.rs` — `map_error`, the `UnknownDevice`
  refusal AC6 pins
- `docs/spec/smart-me-api/openapi-v1.json` — `GET /Devices`: array of `Device`, `200` only;
  `id` uuid non-null, `serial` int64 non-null, `name` nullable. **The description lies about
  casing** (camelCase declared, PascalCase on the wire) — same `rename_all` as `Device`
- `crates/smart-me-client/fixtures/` — story 1.1's capture is the wire truth for shapes

### Previous-story intelligence (3.1–3.3, and Epic 2's close)

- Story 3.2's `body()` lesson: every prior `/healthz` test asserted a status code; assert the
  RENDERED BYTES for anything AC3 promises the operator sees.
- Story 3.1's cadence-test lesson: a mutation that changes nothing observable is not a
  falsification — check each AC7 mutation actually applies before reading its result.
- Story 3.3's failed falsification (`continue` after `step_once` delays nothing) — know what
  your mutation can and cannot affect before running it.
- Epic 2 retrospective actions now BINDING: C1 (pure extraction before close — `decode_devices`
  is this story's instance), C2 (fixtures able to represent change — n/a here unless a test
  compares two listings; then it applies), C3 (falsification = run + copied output, before the
  note), C4 (operator text has a render test), C5 (the description is pinned; re-fetch and diff
  on suspicion, never refresh silently).
- The port-8080 impediment is lifted (2026-08-15) — a local `ci-local.sh` failure on the two
  UI tests is a real failure now, not the documented environment fault.

### References

- [Source: `_bmad-output/planning-artifacts/epics.md:291-306`] — Epic 3, the two decisions of
  2026-08-13 (the second purpose; honesty first) and the 5.3 boundary sentence
- [Source: `_bmad-output/planning-artifacts/prd.md:270,274`] — FR2, FR6 (3.5's, for the
  boundary of what this story must NOT build)
- [Source: [#64]] — the two-readers divergence, its deadline on this story, and why derive is
  the wrong fix
- [Source: `docs/adr/0029-the-declared-serial-is-checked-against-the-one-smart-me-reports.md`]
  — the runtime binding the pick must feed and never replace
- [Source: `docs/adr/0019/0024`] — write-only secrets; same-origin refusals
- [Source: `_bmad-output/implementation-artifacts/epic-2-retro-2026-08-15.md`] — C1–C5,
  adopted and binding

[#64]: https://github.com/guycorbaz/smartme_mqtt/issues/64
[#74]: https://github.com/guycorbaz/smartme_mqtt/issues/74

## Review Findings (2026-08-15, independent pass, fresh context, three angles)

Three finder agents (diff scan, deleted-lines audit, cross-file trace) converged on largely the
same list — the convergence itself corroborating. Triaged: six repairs, four notes, nothing
dismissed silently. Every repair falsified, mutation run before its note.

**REPAIRED:**

1. **The gravest: the client secret could be sent to an arbitrary host in one click.**
   `fetch_listing` took the SUBMITTED `api_base`, and `fetch_token` POSTs the credential to
   `{base}/oauth/token`; `origin::refusal`'s documented curl pass-through (no `Origin` header)
   meant one un-Origined request exfiltrated `SMARTME_CLIENT_SECRET`, persisting nothing.
   Repaired by the repository's own principle read literally — **the file is the configuration**
   (ADR 0023): `discovery_base` reads the SAVED `api_base` (default when absent), so a
   request-supplied base reaches the credential only after being validated and written where
   the operator can see it. The screen says the rule; the signature enforces it.
2. **Enter discovered instead of saving.** The discovery buttons preceded Save in tree order,
   and HTML implicit submission activates the FIRST submit button — the habitual Enter-to-save
   silently fetched, on a page that re-renders almost identically. A hidden leading Save button
   restores Enter-means-Save; the test pins the order.
3. **The discover round trip rewrote mistyped numbers without a word.** `as_typed` is lossy by
   design and the save path pairs it with faults; discover rendered `errors: None` — the
   `publish_period_secs = 0` incident `as_typed`'s own doc memorialises, reopened. Discover now
   validates and renders faults exactly as save does; the test's witness is the fault QUOTING
   the typo (`"8O80"`) after the box was blanked.
4. **The untrimmed credential.** The env values were emptiness-tested trimmed but sent raw, so
   a `docker env_file` trailing newline made discovery render a 401 the bridge itself does not
   have. Trimmed as `config::present` trims.
5. **A second pick duplicated the row**, to be refused at save by a rule the screen never
   names. Picks are idempotent now, and the screen says why nothing was added.
6. **`canonical()` was not injective** — the separators are legal inside every field, so two
   mappings `same_mapping` calls different could hash to one fingerprint (a forgeable guard,
   at the exact moment the story centralised it). Length prefixes disambiguate every boundary;
   the collision is a test's copied output now.

**NOTED, not repaired:**

- The sort key moved from formatted strings to tuples: for names carrying control characters
  below `0x1F`, a confirm page held open across this upgrade answers one fail-closed 409.
  Same one-click cost as the canonical change; recorded in `canonical()`'s doc.
- `classify_device_status` now has a second, id-less caller: a tripwire sentence in its doc
  warns any future id-bearing arm about the "device the collection" message.
- `the_withdrawal_rule_and_the_fingerprint_answer_the_same_question` is narrowed by the
  unification (both sides read one projection); its doc now says what it still guards
  (canonical-vs-equality drift) and where the membership question moved.
- `DISCOVERY_TIMEOUT` was documented as the round's budget while being per-request over two
  requests; halved to 5 s and the doc now tells the truth. Discovery's deliberate lack of
  token reuse/retry-once is recorded in `fetch_listing`'s doc: stateless by decision 2, the
  operator's retry is the retry.

**Falsification of the repairs** — each mutation RUN first: the hidden button removed (RED,
"the hidden leading Save button exists"); `discovery_base` ignoring the file (RED, mirror vs
default); `errors` silenced (RED, `8O80` vanishes from the page entirely — the copied render
shows it); the dedup removed (RED, a third row appears); the length prefix removed (RED, the
two canonicals byte-identical in the output).

### The review of the repair (2026-08-15, same day) — the pattern held a fourth time

The repair commit got its own pass, per story 2.3's rule, and it was wrong four ways:

1. **CONFIRMED, and GitHub caught it first — the repair broke CI.** The new on-screen sentence
   used the word "credential", which the first-run browser test forbids on `GET /config`
   (ADR 0019's mechanical form: `client_secret`, `client_id`, `credential`, `password`). The
   repair had been pushed on lib tests and clippy alone — the full gate ran for the story
   commit and was skipped for its repair, and the process lapse is named in the fix commit.
   Three wordings corrected to the screen's convention ("client id" with spaces, "sign-in"
   for the act), the THIRD found by the new guard, not by reading: the render test now scans
   every discovery outcome for the four tokens, so the next violation fails at the desk.
2. **CONFIRMED — `discovery_base` failed OPEN on an unreadable file**: `.ok()` fell back to
   the default base, sending the sign-in to api.smart-me.com while the operator's saved
   mirror sat unread and the screen claimed the SAVED base was asked. Fail-closed now:
   absence is not invalidity (no file → default), but a file that cannot be read means
   nothing may be asked at all. The residual (an un-Origined client that can POST `/config`
   can save-discover-restore) is stated in the doc as what it is — the UI's no-auth posture,
   not this function's.
3. **CONFIRMED — the dedup's OR made `AlreadyMapped` a lie and blocked the pick's one repair
   use.** Four truthful cases now: exact pair → nothing added, said; ONE row holding half →
   that row's pair is CORRECTED (the account is the authority on which id goes with which
   serial — the transcription repair the pick-list exists for); two rows holding halves →
   nothing moves, the refusal names the rows; no overlap → append. The old lie is in the
   falsification's copied render, verbatim.
4. **PLAUSIBLE, repaired — a pristine first run rendered a page of refusals** for a save
   nobody attempted, while `GET /config` deliberately renders zero faults on the same state.
   The faults now render exactly when the rewrite hazard exists: when something was typed.
5. Also from that pass: the injectivity test gained the ROW-boundary case the original
   finding named and the first test skipped; `canonical()` itself was verified genuinely
   injective (uniquely decodable left-to-right; the unprefixed bool is position-pinned).
   The hidden submit button and the env trimming were checked and cleared.

All four repairs falsified, mutations run first, outputs in hand (the fail-open fallback, the
OR-refuse — whose copied render shows the lie on screen — the unconditional faults, the
dropped length prefixes). Two rounds; the second found different, smaller things than the
first, and the full local gate ran before THIS push.

## Dev Agent Record

### Agent Model Used

claude-fable-5 (the session that closed Epic 2 the same day; C1–C5 binding).

### Debug Log References

### Completion Notes List

**2026-08-15 — the whole story, one sitting, five drafting decisions held.**

- **AC1/AC2's data never meets [#74]'s trap**: the listing deserializes through
  `DeviceListing` (three fields, `Name` optional-and-defaulted), so a meter whose momentary
  reading carries nulls — the unplugged `exterieur` is the live example — stays pickable.
- **The screen is one form with three submit surfaces** (`Save`, `Load the account's meters`,
  per-device `Use this meter`), all reaching their routes via `formaction`. That is what lets
  unsaved edits survive the discovery round-trip without JavaScript, sessions, or any state
  between requests — the handler is a pure function of the submission plus one optional fetch.
- **A pick refetches nothing.** The pair travels in the button's value from the listing the
  operator was just shown; asking the account again would confirm nothing more, and the pair
  is verified where it has always been — against every response, by ADR 0029 (AC6's anchor).
- **[#64] closed by classification, not by derive**: `mapping_projection` decides membership
  field by field with the reasons written, the build stops on any new field (E0027, run), and
  both former readers now read it. `same_mapping`'s behaviour is unchanged by construction —
  sorted-row equality is multiset equality — and the 236-test suite passing untouched is that
  proof, the story 2.1/2.3/2.7 pattern.
- **The `Discovery::Empty` boundary is exact**: empty means no devices AND nothing dropped; a
  listing whose only devices failed to parse renders its caveats, never a shrug.

### Falsification — every mutation RUN before its note (2026-08-15)

| mutation | result |
|---|---|
| AC5: `discovered_name` field planted on `StoredMeter` | BUILD STOPS — `error[E0027]: pattern does not mention field `discovered_name`` at the projection's destructure, exactly the "answer before it compiles" mechanism [#64] asked for |
| AC5: `broker_host` folded into the projection (misclassified as mapping) | RED — *"broker_host is not part of the mapping: changing it must not withdraw a confirmation given for an unchanged meter→topic attribution"* |
| AC5: `enabled` dropped from the row tuple | RED — *"meters[].enabled IS the mapping"* |
| AC1: the drop recorded nowhere (`Err(_) => {}`) | RED — *"the drop is COUNTED, left: 0, right: 1"* |
| AC2: the pick button's value carries the id alone | RED — *"the pick carries device id AND serial together"* |
| AC2/AC4: picking sets `enabled: true` | RED — *"Published stays a deliberate tick, never a side effect of picking"*, the `checked` attribute visible in the copied render |
| (Task 8's run doubles as the regression falsification: 238 bridge + 24 client tests, none edited) | |

### File List

- `crates/smart-me-client/src/types.rs` — `DeviceListing` (AC1)
- `crates/smart-me-client/src/client.rs` — `DeviceList`, `decode_devices`, `get_devices`,
  the collection-404 decision; AC1 tests
- `crates/smart-me-client/src/lib.rs` — exports
- `crates/smartme-bridge/src/app/store.rs` — `mapping_projection` + `MappingProjection`,
  `same_mapping` rebased on it; the membership test (AC5)
- `crates/smartme-bridge/src/ui/screens.rs` — `Discovery`, `discovery_section`, `discover`,
  `fetch_listing`, `form` gains the section; AC2/AC3/AC4 tests; `mapping_fingerprint` rebased
  on the projection
- `crates/smartme-bridge/src/ui/mod.rs` — the `/config/discover` route
- `crates/smartme-bridge/src/adapters/smartme_source.rs` — AC6's anchor on the existing latch
  test
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — status trail
