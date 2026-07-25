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

## D5 — Story 1.4 `Source` seam : trait async dans le core (PARTY MODE — Winston, Amelia, Murat)

**Question :** trait `Source` sync vs async-natif (AFIT/RPITIT) dans le core pur ; forme de
`Reading` ; sémantique du script de `FakeSource`.

**Verdicts unanimes (transcript en annexe B) :**

- **Trait async natif dans le core, forme désucrée `-> impl Future<...> + Send`** (Amelia : la
  borne `Send` explicite est ce qui permet à la task 1.11 générique de traverser `tokio::spawn` ;
  l'AFIT nu ne la donne pas et `trait-variant` = dépendance interdite). L'invariant « no truth
  inside async fn » n'est pas menacé : `Source` est un port qui ne décide rien (Winston).
  Option `block_on` interne rejetée à l'unanimité (panique/deadlock sur worker tokio).
  Un trait sync casserait l'AC de 1.7 (« SmartMeCloudSource impl Source ») et rendrait le chemin
  timeout réel inexerçable — le jumeau 1.14 deviendrait « un jumeau de l'API, pas du chemin
  d'exécution » (Murat).
- **`http_date: Option<UtcMillis>`** : un header absent/imparsable ne doit pas faire échouer un
  fetch — la source n'invente jamais de timestamp, `step` (1.5) tire la conclusion conservatrice.
  `absent` et `malformed` s'effondrent en `None` : même verdict, la distinction diagnostique est
  un log d'adaptateur.
- **Script `VecDeque` ; épuisement → `Err(Fatal{"script exhausted"})`, JAMAIS repeat-last ni
  panic** (FakeSource compile en prod ; « le fake qui répète en silence est le seul design que je
  bloquerais absolument » — Murat). Entrée **`Hang`** distincte (futur `pending`) pour exercer le
  vrai chemin `tokio::time::timeout → Elapsed` sous temps pausé (jumeau 1.14).
- **Helper `poll_now`** (Waker::noop, 1 poll, std pur) accepté comme outillage de test ; confiné
  par arch_purity comme les fakes.

**Arbitrages orchestrateur :**

- `Reading { value: Measurement, http_date: Option<UtcMillis> }` + accesseur `value_date()` —
  la forme de Winston. Amelia proposait `value: Kwh` (sans power) + variante `Malformed` : rejeté
  car contradictoire avec l'AC 1.7 de l'epic (« power converted to Kw » et « the Measurement is
  marked Quality::Bad » — le fail-closed unité passe par la mesure, pas par une erreur).
- `SourceError { Timeout, Transient{reason}, Fatal{reason} }` : reasons `String` gardées
  (diagnostics tracés, jamais parsés pour décider) ; pas de `#[non_exhaustive]` (sans effet
  intra-crate — point d'Amelia).
- Post-review (chasseurs) : side effects du fake déplacés DANS le futur (fidélité au vrai
  `async fn` : un fetch droppé sans poll ne consomme rien) ; introspection
  `remaining()`/`is_exhausted()` pour prouver la consommation complète des scripts ; `poll_now(`
  ajouté aux tokens confinés.

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

## Annexe B — Transcript party mode D5 (extraits clés)

### 🏗️ Winston
« Notre règle dit : aucune vérité n'est décidée dans un async fn. Source::fetch ne décide rien —
c'est un port d'I/O qui rapporte des faits bruts ; la vérité reste dans step(). Confondre "async
dans la signature" et "décision dans l'async" serait du purisme de façade. » « B nous coûte deux
seams au lieu d'un [...] 1.7 devient un mensonge d'AC qu'il faudra renégocier. » « C — rejeté sans
appel : le genre de bombe qu'on découvre en prod à 3h du matin. » « Deux copies de la même vérité,
c'est une invitation à ce qu'elles divergent » (pas de value_date dupliqué). « Exiger http_date
forcerait l'adaptateur à inventer une valeur — c'est précisément mentir. »

### 💻 Amelia
« async fn nu en trait se désucre en RPITIT sans borne Send → E0277 au tokio::spawn générique de
1.11 ; le fix standard est trait-variant, une dépendance → interdit. Donc désucrage manuel avec
+ Send dans le trait. » « &mut self est le bon receiver : la task possède son Source ; l'asymétrie
avec FakeClock (Mutex, &self) est voulue. » « Script vide = Err, jamais panic — FakeSource est
compilé en prod comme FakeClock ; répéter le dernier Ok simule des données fraîches à l'infini et
masque exactement le scénario STALE de 1.14. » « Symétrie obligatoire : token "FakeSource" dans la
deny-list d'arch_purity. »

### 🧪 Murat
« On juge une seam par les tests qu'elle rend triviaux ET par les mensonges qu'elle rend
impossibles. » « Avec un trait sync, le jumeau 1.14 devient un jumeau de l'API, pas du chemin
d'exécution — pour un système "never lies", trou de couverture de classe A. » « Épuisement →
Fatal typé, bruyant, et une politique de fin explicite : le silence doit être scripté, jamais
accidentel. » « Timeout instantané suffit pour 1.5 ; pour la revendication jumelle de 1.14 il faut
EN PLUS une entrée Hang : sous start_paused, tokio::time::timeout expire vraiment et on exerce le
chemin Elapsed → Timeout → STALE de production. »

## D6 — Story 1.5 : `Failed` absorbant + planchers (orchestrateur, sur findings de revue)

- **`Failed` est absorbant jusqu'au restart.** Les chasseurs ont montré que `prev` était un
  paramètre mort : après une erreur fatale (auth rejetée), un simple timeout re-publiait `Stale`
  (blanchiment de `Bad`) et un Ok re-passait `Fresh` en silence. Incohérent avec ADR 0009
  (« stop + surface ») et la config restart-only. Tranché : `prev == Failed → (Failed, Bad)`,
  première ligne de la table ; seul un restart (nouvel `initial()`) rouvre la porte.
- **`Fatal` jugé avant le garde d'horloge de boot** : une RTC désynchronisée ne doit pas adoucir
  une erreur d'auth (indépendante de l'horloge) en `Stale`.
- **Le plancher 2020 s'applique aussi à `http_date`** : une paire cloud cohérente datée 1970
  n'est pas une lecture vivante.
- **Gardés en spec-litéral (différés Epic 2, consignés)** : oracle anti-feed-gelé (monotonie de
  `http_date` inter-ticks), tolérance de troncature ±1 s, validation de `max_age_ms`.
- **Rejeté explicitement** : hystérésis/debounce — la démotion instantanée EST le choix « when in
  doubt, STALE » (bruit d'alarme accepté, mensonge refusé).

## D7 — Story 1.6 client HTTP : stack verrouillée + déviations assumées (orchestrateur)

- **Pas de party mode** : l'architecture verrouille déjà reqwest/serde/thiserror. Arbitrages :
  backend TLS = **rustls + webpki-roots** (pas de native-tls/openssl dans l'arbre ; le conteneur
  n'a pas besoin de ca-certificates) ; features reqwest 0.13 renommées (`rustls`, `form`, `http2`).
- **Licence CDLA-Permissive-2.0 ajoutée à deny.toml** : données du root-store Mozilla CCADB dans
  webpki-root-certs — licence de données permissive standard, commentée dans le fichier.
- **AC3 sans mock HTTP** : le client refuse le plaintext par construction ; le contrat est prouvé
  sur les OCTETS réels des fixtures avec les mêmes type-parameters serde que `get_device`
  (y compris l'enveloppe objet de `/Devices/{id}`, fixture générée de la capture réelle après que
  la revue a montré le trou liste-vs-objet). Un mock TLS self-signed aurait testé reqwest, pas
  notre code.
- **Classification transient/fatal exportée par le crate** (`is_fatal()`) : 400 du token endpoint
  fatal SEULEMENT si le corps OAuth (RFC 6749) accuse le client — un artefact WAF ne doit pas
  verrouiller le `Failed` absorbant de 1.5 (D6). Sentinelle `status: 0` remplacée par une
  variante `Misconfigured`.
- **NFR12 durci sur flag unanime des 3 couches** : Debug manuels rédigés (`<redacted>`) sur
  Credentials/TokenState/SmartMeClient + test anti-fuite ; redirections coupées
  (`Policy::none()` — un 307 ne rejouera jamais le formulaire du secret vers un autre hôte).

## D8 — Stories 1.9/1.10 : device-level Sparkplug MAINTENANT (PARTY MODE — Winston vs Murat, arbitrage orchestrateur)

**Question :** l'AC 1.9 dit « the device is keyed by Serial » et l'architecture « per-meter device
keyed by Serial », mais la Story 1.8 avait DIFFÉRÉ les messages device-level (D*) et la
construction de topics. Trois options : (A) node-level, identité du compteur dans le NOM de
métrique ; (B) ajouter le device-level à `sparkplug-b` maintenant ; (C) node-level + amendement
formel de l'AC via correct-course.

**Désaccord franc du panel :**
- **🏗️ Winston → C.** « Le coût réel n'est pas de rouvrir un crate — c'est rouvrir un modèle de
  séquencement que je viens de stabiliser. [...] Construire le mécanisme le plus subtil sans
  oracle, c'est de l'architecture à l'aveugle. » Il reconnaît la discontinuité d'historisation
  mais la juge sans valeur métier en Epic 1 (« quelques semaines sur un seul compteur »).
- **🧪 Murat → B.** (1) l'assertion du jumeau chaos devient structurelle (filtrage par le broker)
  au lieu d'un string-splitting qui « passe au vert quand la sémantique est fausse » ; (2) le
  NDEATH est per-node : sans device-level, aucune granularité protocole pour dire « compteur 3
  STALE, les 3 autres GOOD » ; (3) asymétrie de risque : coût borné maintenant vs coût
  irréversible côté données plus tard ; (4) seul B donne au test manuel 1.15 un pouvoir de
  falsification réel.

**Arbitrage : B.** L'argument (2) de Murat porte sur 4 compteurs — hors périmètre de l'Epic 1
(un seul compteur), donc il ne tranche pas ici. Ce qui tranche est son point (3) : la grammaire
de topic est un CONTRAT versionné et migrer après historisation SCADA casse la continuité des
courbes — coût irréversible payé par l'utilisateur, contre un coût borné aujourd'hui sur un crate
que personne d'autre ne consomme. L'architecture spécifie déjà le device-level : l'option C
demanderait d'amender une décision d'architecture verrouillée, acte de gouvernance plus lourd
qu'écrire DBIRTH/DDATA. Livré : `topic.rs` (EdgeNode validé, niveaux vérifiés en release aussi),
`device_birth`/`device_data`/`device_death` partageant le `seq` de l'edge node.

**Post-revue (défauts structurels corrigés, pas documentés) :**
- Émission partielle : un serial illégal émettait le NBIRTH puis échouait → tous les topics sont
  validés AVANT la première émission ; sur erreur, rien n'est parti et la session est intacte.
- Le rebirth écrasait les valeurs connues par du Null/Stale à chaque reconnexion (blip transport
  = trou d'historique) → le publisher mémorise la dernière lecture par device et la re-déclare.
- Drop silencieux (`Ok(())`) contraire à l'exigence « traced drop, never silence » → issue typée
  `Published { Emitted, DroppedBeforeBirth, DroppedUndeclaredDevice }`.
- `CONTRACT_VERSION` était une constante morte — contradiction directe avec la justification de
  D8 → publiée dans le NBIRTH (`Contract/Version`), un consommateur VOIT le changement de contrat.
- Garde de niveau de topic en `debug_assert!` seulement (absent en release, là où ça compte) →
  `Result` avec une variante `WrongLevel`.
