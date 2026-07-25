# Sprint Epic 1 — Journal des décisions (run autonome 2026-07-25)

Chaque décision prise pendant le run autonome est consignée ici : contexte, options,
qui a tranché (party mode / orchestrateur / pré-approbation de Guy), et verdict.

## D1 — Régime de commits du run (Guy, AskUserQuestion)

**Décision :** commit + push par story terminée, en référençant l'issue GitHub de la story.
Dérogation explicite à la règle « jamais de commit sans accord » pour la durée du run.

## D2 — Story 1.1 exécutée avec les credentials réels (Guy, AskUserQuestion)

**Décision :** capture réelle autorisée avec le `.env` local (lecture seule), fixtures
anonymisées (`Id`/`Serial`/`Name`) avant commit.

## D3 — Anonymisation & périmètre des fixtures 1.1 (orchestrateur)

- Seuls les champs d'identité sont scrubbed ; valeurs électriques et timestamps verbatim
  (leur réalisme est le but de la capture). Vérifié par script (zéro fuite).
- Les 4 devices sont conservés — dont le compteur mort depuis 96 jours (cas STALE naturel).
- Headers Cloudflare de télémétrie (`report-to`/`nel`/`cf-ray`) exclus du `valid.txt` commité
  (corrélables au compte, inutiles à l'oracle).

## D4 — Story 1.3 `Clock` seam : représentation du temps (PARTY MODE — Winston, Amelia, Murat)

**Question :** (1) `std::time::Instant` opaque vs newtype `MonotonicMs(i64)` ;
(2) étendre `arch_purity` au ban textuel de `Instant::now()`/`SystemTime::now()` ;
(3) emplacement de `FakeClock`.

**Verdicts unanimes du panel (transcript intégral en annexe A) :**

- **V1 — `MonotonicMs(i64)`.** `Instant` est disqualifié mécaniquement : infabricable sans
  `Instant::now()`, précisément l'appel banni par l'AC — un fake qui doit tricher pour exister
  importe du non-déterminisme dans le harnais (Murat : « le pire endroit possible »).
  `MonotonicMs` garde le tick 1.5 en données plates copiables et rend triviaux les 7 scénarios
  de test listés par Murat (frontière au ms, chaos localisé, NTP backward, valeurs extrêmes,
  proptest, fake-n'avance-jamais-seul).
- **V2 — Oui, étendre `tests/arch_purity.rs`** : tokens `Instant::now(`, `SystemTime::now(`,
  `use std::time::Instant`, `use std::time::SystemTime` (fermeture du contournement par alias,
  Amelia) bannis dans `src/**`, exemption unique `core/clock.rs`. Le ban couvre aussi les mods
  `#[cfg(test)]` inline — jugé feature, pas bug (un test non-déterministe du code déterministe
  est une contradiction, Murat).
- **V3 — `FakeClock` en `pub` normal dans `core::clock`** ; `#[cfg(test)]` éliminatoire
  (invisible aux tests d'intégration `tests/staleness_injected_clock.rs`), feature flag
  `test-util` disproportionnée (matrice CI, self-dev-dependency, double compilation sous
  `jobs=2`). Garde-fou : token `FakeClock` banni par arch_purity dans `src/**` hors
  `core/clock.rs`.

**Arbitrages orchestrateur (désaccords internes du panel) :**

- Emplacement de `MonotonicMs` : Amelia le voulait en `domain` (symétrie `UtcMillis`), Winston
  frappé-uniquement-par-clock. Tranché : **`core/clock.rs`** — c'est un concept runtime
  process-local, pas un concept de mesure wire ; `core` est scanné par arch_purity donc la
  pureté est identique, et la machine d'états 1.5 (dans `core`) le consomme sans détour.
- Champ **`pub i64`** (Amelia) plutôt que privé (Winston) : miroir exact de `UtcMillis(pub i64)`,
  fabricabilité requise par les property-tests ; la provenance en prod est déjà garantie par le
  ban V2 (aucun moyen d'obtenir du temps réel hors `Clock`).
- `FakeClock` sur `AtomicI64` + `&self` (Amelia) — `Sync` d'emblée pour le partage `Arc` des
  stories 1.5/1.11 ; `advance_ms` avance **les deux** horloges (réalisme), `set_wall` permet le
  saut NTP isolé.

## Annexe A — Transcript party mode D4

### 🏗️ Winston

L'option A est séduisante parce que « std, c'est boring ». Mais c'est un faux boring.
`Instant` est opaque par conception : on ne peut ni le construire, ni le sérialiser, ni le
fabriquer en test sans appeler `Instant::now()` quelque part — précisément ce que l'AC
interdit hors de `SystemClock`. [...] `MonotonicMs(i64)` est cohérent avec ce qu'on a déjà :
`UtcMillis(pub i64)`, même granularité, même style de newtype, `Copy`, comparable,
soustraction saturante triviale. Le tick struct de 1.5 reste un POD copiable. [...]
L'objection sérieuse : la monotonicité n'est garantie que par provenance — j'assume ce
trade-off et je le rends structurel. [...] Extension arch_purity : oui, sans hésiter — le
coût marginal est de dix lignes ; un faux positif se corrige en trente secondes, le faux
négatif d'une review humaine coûte un bug d'horloge en production chez Ignition. [...]
FakeClock : `pub` dans `core::clock`, tout simplement — nous construisons un binaire interne,
pas une lib publique ; le feature flag n'achète aucune valeur métier. « Le temps est une
donnée frappée à un seul endroit, plate partout ailleurs, et l'invariant est vérifié par la
machine, pas par la vigilance. »

### 💻 Amelia

V1 non négociable : option A est morte à l'AC2 — pas de const ctor, pas de
`Instant::from_millis`. Un fake qui viole la règle qu'il est censé faire respecter, c'est non.
Signatures : `trait Clock { fn monotonic(&self) -> MonotonicMs; fn wall(&self) -> UtcMillis }` ;
`SystemClock { start: Instant }` privé ; conversions `try_from().unwrap_or(i64::MAX)`, jamais
`as` ; object-safe (`Arc<dyn Clock + Send + Sync>`) ; i64 pas u64 (cohérence `Sub -> i64`,
292 M d'années d'uptime avant overflow). V2 : quatre tokens, pas deux — fermer l'alias
`use std::time::Instant as I`. Faux positifs analysés : doc-comments skippés, pas de
self-hit (arch_purity scanne `src/**`, vit dans `tests/`). V3 : `#[cfg(test)]` éliminé
d'office ; feature = self-dev-dependency + double compilation sous `jobs=2` ; `#[doc(hidden)]`
= obscurité, pas sûreté. `FakeClock { mono: AtomicI64, wall: AtomicI64 }`, `advance_ms` avance
les deux, `set_wall` pour le saut NTP. Et une ligne arch_purity de plus : token `FakeClock`
banni hors `core/clock.rs` — « un fake qui fuit vers app/infra casse la CI, pas juste la
review ».

### 🧪 Murat

Règle de décision : quel choix rend les scénarios à haut risque triviaux à écrire et
impossibles à tricher. Option A : un FakeClock qui capture un Instant réel puis offsette
importe du non-déterminisme dans le harnais — probabilité faible × coût de diagnostic énorme
= inacceptable pour un bridge « never lies ». Sept scénarios que la représentation doit rendre
triviaux (≤ 3 lignes de setup chacun) : frontière au seuil ±1 ms ; chaos 1.14 localisé
(timeout cloud = advance de 3× l'intervalle) ; NTP backward (wall recule, monotone avance,
le verdict ne rajeunit pas) ; huge_skew +1 an ; extrêmes 0 et i64::MAX−ε (saturation, pas de
panic) ; proptest sur la machine 1.5 (le temps doit être générable) ; deux lectures sans
`advance()` strictement égales. V2 : sur un projet solo, la review-only est une garantie qui
vaut zéro à 2h du matin ; le modèle de menace est le développeur pressé de bonne foi, pas
l'adversaire malin — pas d'analyseur AST pour un outil 4 mètres. Bannir `now()` dans les
`#[cfg(test)]` inline de src/ est un feature. V3 : pub + `#[doc(hidden)]` si ça démange, gardé
par le scan V2. « Les trois verdicts s'appuient l'un sur l'autre — B rend le scan V2
suffisant, et le scan V2 rend le pub V3 sûr. »
