# Martuni — Roadmap

Zentrale Übersicht der nächsten Schritte und der bisherigen Maßnahmen.
Detail-Begründungen, Mess-Verläufe und Konzepte stehen in den verlinkten
Einzeldokumenten:

- Search: [lmr-plan.md](lmr-plan.md), [null-move-pruning.md](null-move-pruning.md), [see.md](see.md)
- Evaluation: [eval-kalibrierung.md](eval-kalibrierung.md), [endgame.md](endgame.md), [vorbereiteter_Prompt_dynamische_Figurenbewertung.md](vorbereiteter_Prompt_dynamische_Figurenbewertung.md)
- Tooling: [blunder-analyse.md](blunder-analyse.md)

## Aktueller Status

**19.06.2026 — Staged MovePicker (Code-Review-P1): `order_moves` (eager: alle Züge
scoren → `Vec<ScoredMove>` → Gesamt-Sort pro Knoten) ersetzt durch lazy `MovePicker`
mit 8 Stufen (TT → stille Dame-Umwandlung → gewinnende Captures → Killer 1/2 →
Unterumwandlung → ruhige Züge → verlierende Captures). BIT-EXAKT, +8–12 % NPS.**
- **Gewinn-Mechanik:** Ein früher Cutoff (typisch TT-Move an Cut-Nodes) überspringt die
  SEE-Berechnung *aller* Captures und beide Sorts komplett — die teure Arbeit fällt nur an,
  wenn ihre Stufe erreicht wird.
- **Bit-Exaktheit (zwei Fallen sauber adressiert):** (a) SEE wandert ans Stufenende, ist aber
  reine Funktion der (während des Knotens unveränderten) Stellung → wertgleich; (b) die
  Quiet-History-Scores werden in `MovePicker::new()` bei Knoten-Eintritt gelesen, BEVOR
  Kind-Suchen die globale History-Tabelle mutieren → nur das *Sortieren* der Quiets ist
  verzögert, die Reihenfolge bleibt identisch. Klassifikation in `new()` folgt exakt der
  alten if-else-Priorität → jeder Zug landet in genau einer Kategorie, kein Doppel-Ausgeben.
- **Verifikation:** Node-Counts über 8 diverse Stellungen × alle Tiefen identisch zur Baseline
  (deterministisches Harness, Buch + Syzygy bewusst aus → reine Alpha-Beta); 94/94 Tests grün;
  NPS +8–12 % (best-of-6, identische Knoten, unter Turnier-Last gemessen). Methodik wie
  Hotpath-Cleanup Bundle 1 — kein A/B nötig, da bit-exakt.
- **Stand:** committet auf Branch `dev/engine-arbeit` (Dev-Worktree `../enginemartuni-dev`).
  Live-Binary unberührt — **Rollout (Merge → `master`, Live-Build, `lichess-bot.service`-Neustart)
  erst nach dem laufenden 12h-Bullet-Turnier**, um keine Partie zu unterbrechen.

**15.06.2026 — Syzygy-Tablebases (3-4-5) via `pyrrhic-rs`: Phasen ①–④ + Integritäts-Guard
(Adapter, Option/Config, WDL-in-Suche, DTZ-Wurzel), end-to-end validiert, GEPUSHT
(①–③ `499e22c`, ④+Guard `dc974a1`). AKTIVIERT + LIVE seit 15.06. 18:34 CEST
(Tobias-Entscheid, kein A/B — Tabellen millionenfach erprobt).**
- **Auswertung `analyse-15.06.2026.json` (159 P, 294 Blunder, B/P 1.849):** Der Sprung von
  1.439 (08.06.) ist **kein Regress, sondern Opponent-Mix** — AetherBot (25 P, B/P 2.8) +
  stickshark99 (25 P, 2.4) stellen 31 % der Partien, aber 44 % der Blunder; Rest ~1.4.
  Bei bit-exaktem Live-Binary (14.06.) ohnehin nicht regressionsfähig. EG-Anteil erhöht
  (94 Blunder, 0.591/P), aber Gros in **8–14-Steine**-Stellungen; nur **6/294** in ≤5 Steinen
  → Syzygy ist **kein direkter B/P-Hebel** für dieses Fenster, sondern Korrektheits-/Polish-Lever
  (perfekte Konversion, retired die fragile Endspiel-Heuristik für ≤5 Steine).
- **Rating 15.06.: Blitz 2037 (−68 vs 14.06.) / Rapid 2205 (flat).** Setzt das 13.06.-Muster
  fort (Blitz volatil/schwächeres Format, Rapid stabil); Binary seit 14.06. bit-exakt + reine
  Korrektheits-Fixes → kein Eval-Regress, Blitz −68 = Tagesvarianz.
- **Entscheid (Tobias):** Syzygy **3-4-5 via `pyrrhic-rs`** (FFI um Pyrrhic/Fathom), heutiger
  Umfang Phasen ①–③ (WDL in der Suche), DTZ-Wurzel nächste Sitzung. Doc:
  [syzygy-rust-options.md](syzygy-rust-options.md).
- **Umgesetzt (uncommitted):**
  - `src/syzygy.rs` (neu): `ChessAdapter` (EngineAdapter-Trait → `chess`-Angriffstabellen,
    Index→Square), `Syzygy::load`/`probe_wdl_score`. Gates: Steine ≤ max_pieces, **keine
    Rochaderechte**, **kein en passant** (v1 — `chess`-EP-Semantik ≠ Fathom-Zielfeld, Auslassen
    ist immer korrekt). WDL→Score: Win `90_000−ply` / Loss `−(…)` / Draw·Cursed·Blessed `0`
    (unter `MATE_THRESHOLD` 99_000 → echte Matts gehen vor, TT-Mate-Normierung unberührt).
  - `options.rs`/`config.rs`/`uci.rs`: UCI-Option `SyzygyPath` + `.env`-Key `SYZYGY_PATH`,
    Handle-Laden bei Start/setoption, `Option<Arc<Syzygy>>` in `SearchRequest`.
  - `search.rs`: WDL-Probe im inneren Knoten (ply > 0, nach 50-Züge/Repetition-Check) als
    Cutoff; `tbhits` im `info`-Output (nur wenn > 0 → Default-Output byte-identisch).
- **Verifikation:**
  - **89/89 Tests grün** (86 + 3 neue in `syzygy.rs`: Index-Mapping, Adapter=chess-Angriffe, Gates).
  - **Bit-Exaktheit (default-off):** Referenz aus `HEAD` (Worktree, md5 27acb202 = Live) vs.
    WIP-Binary, **8 diverse Stellungen** `go depth 6` deadline-frei — **identische Tiefe +
    Node-Count + bestmove** (beide aus Projekt-Root, damit gleiche eval.toml). Methodik-Falle
    notiert: Binary aus fremdem CWD findet eval.toml nicht → Default-Eval → scheinbarer Mismatch.
  - **End-to-end WDL (kompletter Satz):** KBNvK tbhits=368 cp 89999 (Win — das ursprüngliche
    Mop-up-Motiv jetzt TB-gelöst), KRvK/KQvKR Win, KQvKQ/KRvKR cp 0 (Remis), 5-Steiner
    (KRPvKR) probt. Keine Crashes.
  - **End-to-end DTZ-Wurzel (Phase ④, Playout beide Seiten engine+TB):** KBNvK **mattet in 55
    Halbzügen** bei optimaler Verteidigung (< 100-Ply-50-Züge-Grenze — genau die Remis-Klasse,
    die die alte Eck-Heuristik verlor), KRvK 23, KQvKR 27, alle 1-0 echtes Schachmatt
    (`board.is_checkmate()`). Die **Gewinnerseite trifft die DTZ-Wurzel zu 100 %**; die
    verlierende (verteidigende) Seite fällt auf die normale Suche zurück (`root` dort kein
    `DtzResult`) — harmlos, weil der ply-adjustierte WDL-Score `-(TB_WIN−ply)` automatisch
    längsten Widerstand spielt.
  - **Integritäts-Guard verifiziert:** SyzygyPath auf Ordner mit absichtlich korrupter Datei →
    `info string Syzygy: deaktiviert — 1 defekte… KQvK.rtbw`, **kein SIGBUS**, Engine sucht
    normal weiter. Plus Unit-Test `verify_tables_flags_bad_magic`.
- **Tabellen:** voller 3-4-5-Satz **290 Dateien / 940 MB** in `~/syzygy/3-4-5` (sesse.net;
  rekursiver Crawl wird 403-geblockt → Einzel-GETs via `wget -i` + UA + Nachlade-Reparatur;
  alle 290 magic-byte-verifiziert WDL `71e8235d` / DTZ `d7660ca5`).
- **KRITISCHER BEFUND (jetzt entschärft) — truncierte Tabellen → SIGBUS (nicht von
  `catch_unwind` fangbar):** Eine abgeschnittene `.rtbw`/`.rtbz` (mmap) crasht die Engine beim
  (rekursiven) Probe → Bot-Forfeit. Aufgetreten mit 403-Trunkaten im Smoke-Ordner (KQvKQ→KQvK).
  **Lade-Zeit-Guard `verify_tables` ergänzt:** prüft alle `.rtbw`/`.rtbz` VOR dem mmap auf
  Magic-Bytes; ein Defekt → Tablebases ganz aus (`None`) statt Crash. Best-effort (Magic +
  Lesbarkeit; exakte Seitengrenzen-Trunkate mit gültigem Header bleiben unentdeckt → die
  einmalige Magic-Verifikation des Download-Ordners bleibt die autoritative Prüfung).
- **Nebenbefund:** `pyrrhic_rs::max_pieces()` meldet **7** (Capability-Konstante, obwohl nur
  3-4-5 geladen) → Gate lässt 6-7-Steine-Knoten zur Probe durch; Fathom liefert dort sauber
  FAILED (TB_LARGEST=5) → korrekt, nur minimal verschwendete Probe-Calls. Optionale Verfeinerung:
  echte max-Kardinalität aus den Dateinamen ableiten und kappen.
- **ROLLOUT 15.06. 18:34 CEST (Tobias, kein A/B):** `SYZYGY_PATH=/home/librechat/syzygy/3-4-5`
  in der Projekt-`.env` gesetzt (Engine findet sie via Kaskaden-Suche aus der Bot-CWD `~/lichess-bot`,
  wie die Bücher — Pre-Flight bestätigt: `Syzygy: tablebases loaded (up to 7 men)`). Live-Binary
  = `martuni.syzygy_phase4_20260615` (md5 **5f8b9aa8**) → `target/release/martuni`. Bot-Neustart
  bei idle (keine Partie betroffen, letzte endete 16:45), `Engine configuration OK` + `Welcome
  Martuni / connected`. **90/90 Tests grün.** Backups: `…syzygy_wip…` (①–③ fc889864), approved
  Vor-Syzygy `27acb202` (rekonstruierbar via Worktree `a673ee2`).
  - **Rollback** (falls nötig): `SYZYGY_PATH` aus `.env` entfernen + Bot-Neustart (Code dann
    bit-exakt zur Vor-Syzygy-Engine), ODER Binary zurück auf `27acb202`.
- **Offen:** (1) **Lichess-Lookback** gegen Anker 15.06. (Blitz 2037 / Rapid 2205) — KPI:
  Endspiel-Konversion ↑ (verschenkte ≤5-Steine-Remis → 0), kein Rating-Einbruch; (2) optional
  `max_pieces`-Kappung (meldet 7 statt 5 — harmlos, nur verschwendete Probe-Calls an
  6-7-Steine-Knoten); (3) optional en-passant-Behandlung statt v1-Skip.

**14.06.2026 — Hotpath-Cleanup Bundle 1 aus dem grok-/Cursor-Auto-Effizienz-Review umgesetzt
(bit-exakt, kein Verhaltenswechsel). A/B 1000 P kein Regress. Branch `perf/eval-hotpath-cleanup`.**
- **Review-Verifikation:** Befunde des `code-review-effizienz-cursor-auto-2026-06-13.md` vor der
  Umsetzung gegen den echten Quellcode geprüft → **echt** (Zeilennummern, Funktionsnamen, Duplikate
  stimmten). Konkurrierende Hermes-Behauptung „redundante `game_phase`-Aufrufe in `evaluate()`" war
  **falsch** (Phase wird genau einmal berechnet und durchgereicht) → bewusst nicht umgesetzt.
- **Bundle 1 — 5 bit-exakte Maßnahmen (Commit `271f9f3`), reine Geschwindigkeit/Entdopplung/Dead-Code:**
  1. Totes Feld `root_best_score` aus `search.rs` entfernt (geschrieben, nie gelesen).
  2. `is_passed` vereinheitlicht — bit-identische Kopie `is_passed_simple` aus `search.rs` gelöscht,
     `eval::is_passed` jetzt `pub(crate)` (eine Quelle der Wahrheit statt zwei driftgefährdeter Kopien).
  3. Chebyshev vereinheitlicht — lokale `eval_chebyshev` gelöscht, `endgame::chebyshev` jetzt `pub(crate)`.
  4. `#[inline]` auf ~14 kleine Hot-Helper (vorher nur `taper`) — reine Hints, keine Ergebnisänderung.
  5. Wurzel-`MoveGen` 3× → 1× pro `go` — legale Wurzelzüge einmal erzeugen, für Forced-Move-Check +
     Fallback wiederverwenden (deterministisch → bit-identisch, spart zwei MoveGen-Läufe je Suche).
- **Verifikation:**
  - **`cargo test --release`: 86/86 grün.**
  - **Bit-Exaktheit (stärkstes Gate):** an deadline-freiem `go depth 8` liefern Baseline (master) und
    Kandidat für 12 diverse Stellungen **identischen bestmove UND identischen Node-Count**; das Aggregat
    von **195.020.958 Knoten** ist über alle Läufe beider Binaries deckungsgleich → beweisbar keine
    Verhaltensänderung.
  - **NPS:** −0,82 % Median (im Rauschen des geteilten Live-Bot-Servers; Node-Counts identisch → reine
    Zeitvarianz).
- **A/B-Selfplay (5+0.05, UHO_Lichess, Hash=64, conc=2), 1000 Spiele gepoolt:** W434 / L407 / D159 →
  Baseline 51,35 % → **+9,4 Elo ±~17 (95 %-CI deckt 0), LOS ≈86 % — nicht signifikant** (Pilot 300:
  +32,5/LOS 96,9 % war ein ~1,85σ-Rauschblip; Erweiterung 700: −0,5/LOS 48 % flach) → **kein Regress**,
  neutral wie von der Bit-Exaktheit vorhergesagt.
- **Lichess-Rating-Snapshot (vor Merge, Lichess-API, 2026-06-14T10:39:53Z):**
  **Blitz 2105** (2276 Partien) / **Rapid 2205** (1350 Partien).
- **Nutzen:** kein Elo-Versprechen (bit-exakt → Stärke unverändert); Wert = Entdopplung (killt das
  „Bugfix-nur-an-einer-Stelle"-Risiko bei `is_passed`/Chebyshev), Dead-Code-Abbau, kleineres NPS-Polster.
  PR gegen `master` offen.

**13.06.2026 — Rückschau: TT-Mate-Fix bestätigt (Mop-up-Remis 6 → 0). KBNvK-Restdefekt
gefunden & gefixt (Center-Distanz-Gradient, Option A). Rollout = Tobias-Entscheid offen.**
- **Rollout-Status TT-Mate-Fix verifiziert:** Live-Binary `c92b1241` seit Bot-Neustart
  **10.06. 21:43 CEST** aktiv. `analyse-13.06.2026.json` = 251 Partien, davon **250 post-Fix**
  (10.06. 19:43 UTC – 13.06. 15:34 UTC) → sauberes Rückschau-Fenster.
- **Verschenkte Mop-up-Remis 6 → 0:** Replay aller 33 post-Fix-Remis (Endstellungs-Material
  via python-chess). Genau **1** Remis mit ≥ Turm-Vorteil (BaymaxMate `EGfWKKZ2`) — aber
  **Mess-Artefakt**: Endstellung war K+T vs K+T (echtes Remis), Turm erst im letzten Zug
  (105. Rxb6) geschlagen; echte Tiefen d23–d28, Eval 0.00, Uhr normal → **kein d1/0-ms-Fossil
  mehr.** Die vom Fix anvisierte 6-Remis-Klasse ist verschwunden. **KPI erfüllt.**
- **Rating:** 13.06. **Blitz 2044 / Rapid 2207** (Anker 10.06. Blitz 2100 / Rapid 2176).
  Rapid **+31** (Fix wirkt sauber), Blitz **−56**. Schere = Format-Stärke, kein Eval-Regress:
  Score post-Fix nach Lichess-Kategorie **Rapid 64,4 % / Blitz 54,8 % / Bullet 41,2 %**.
  Time-forfeits netto **positiv** (10 S / 2 N / 1 R) → nicht der Treiber.
- **Restdefekt (sekundär, vom Fix freigelegt): schwacher Mop-up-Gradient.** Beispiel sxphia
  `u6N2u9an`: Martuni sieht **+7,89 bei echtem d27**, fällt aber **auf Zeit** (180+0) beim
  Eck-Schieben statt zu matten — der Nebenbefund `eg_corner_weight` aus dem 10.06.-Report.
  **Mate-Probe (Live-Binary `c92b1241`, validierte Stellungen):** KR/KQ konvertieren sauber,
  aber **KBNvK 3/6 Remis** + Matt zu langsam (79–93 Hz). Ursache: `(7 − corner_d)` mit
  Chebyshev-zur-nächsten-Ecke plateaut (d5 = a5 = 3) → König irrt zur falschen Ecke.
  Scope-Caveat: `mop_up_score` feuert nur bei reinen Bare-King-Signaturen (keine Bauern) →
  **Korrektheits-Fix, Lichess-Rating-Wirkung klein** (KBNvK selten; sxphia = K+B+P fällt
  nicht darunter).
- **Fix gebaut — Option A (Center-Distanz, CPW-Standard, Tobias-Entscheid):**
  - `endgame.rs` `mop_up_score` (KR/KQ/KRR): `(7 − corner_d)` → `center_manhattan_distance`
    (0 Zentrum .. 6 Ecke, streng monoton). Neue Helfer `manhattan` /
    `nearest_manhattan_distance` / `center_manhattan_distance`; `nearest_corner_distance`
    + `ALL_CORNERS` entfernt.
  - `kbnk_score`: `(7 − corner_d)` → `(14 − nearest_manhattan_distance)` zur läuferfarbenen
    Zielecke (kein Plateau, richtet Gradient auf die RICHTIGE Ecke).
  - **Tests: 86 grün** (2 neu: `center_manhattan_distance_zero_at_center_six_at_corner`,
    `kbnk_gradient_pulls_to_bishop_colored_corner`; `connected_rooks` in eval.rs an neue
    Formel angepasst: KRRvK d8 → CMD 3 → 1120 statt 1140).
  - **Mate-Probe gegen Variante: KBNvK 0/6 Remis** (alle matten, 51–63 Hz statt 93/Remis),
    KR/KQ unverändert. Smoke grün (Buch + Mittelspiel normal).
- **Binary-Hygiene:** Variante = `martuni.mopup_grad_20260613` (md5 `6bfc392d`).
  `target/release/martuni` auf approved `c92b1241` zurückgesetzt → **Live-Bot unverändert**,
  kein stiller Rollout bei systemd-Restart. Source-Änderungen uncommitted im Tree.
- **Offen (Tobias-Entscheid):** (1) Rollout direkt wie TT-Mate-Fix (commit + Variante live +
  Neustart, kein A/B nötig — Bug-Fix-Kategorie, Mate-Probe ist die Validierung), oder
  (2) A/B als No-Regression-Guard (~1000 P, fast sicher flat), oder (3) nur committen.
  Größerer Blitz-Hebel bleibt allgemeine Endspiel-Technik (K+Bauern), nicht der Bare-King-Term.

**10.06.2026 (Fix) — TT-Mate-Ply-Bug behoben (Option C = Adjustment + Matt-Break-Guard).
Tobias-Entscheid; Rollout = Bot-Neustart 10.06. 21:43 (verifiziert 13.06.).**
- **Fix A — Mate-Ply-Adjustment** (`search.rs`): neue Helfer `mate_score_to_tt` /
  `mate_score_from_tt` — Mate-Scores werden beim `tt.store` knotenrelativ normiert
  (`score ± ply`) und beim Probe-Cutoff auf die Wurzel der aktuellen Suche zurückgerechnet.
  Normale Scores passieren unverändert. Standardlösung jeder TT-Engine.
- **Fix B — Matt-Break-Guard:** der Deepening-Abbruch bei gefundenem Matt greift nur noch,
  wenn die Mattdistanz innerhalb der gerade abgeschlossenen Suchtiefe liegt
  (`MATE − |score| ≤ depth`) — ein TT-gestütztes „mate N" jenseits der Tiefe wird
  weitergerechnet statt bei d1 blind gespielt. Kostet praktisch nichts.
- **Nebenprodukt:** `SearchResult.score` (Score der letzten abgeschlossenen Iteration,
  im Bin-Target ungenutzt) für Tests/Diagnostik.
- **Tests: 84 grün** (82 + 2 neu): `mate_score_tt_normierung_roundtrip` (konkrete
  Zahlen, beide Vorzeichen, Identitäts-Roundtrip) und `tt_mate_distance_shrinks_across_
  searches` (KQvK, geteilte TT, zwei Plies weiter → Distanz MUSS schrumpfen).
  Sabotage-Gegenprobe: mit kurzgeschlossenen Helfern wird der Test rot — er beißt.
- **End-to-End-Verifikation** (deterministischer zKfpQEn8-Replay, echte Uhrstände):
  vorher ab Zug 83 nur 0-ms-d1-Antworten mit eingefrorenem „mate 15"; nachher schrumpft
  die Distanz Zug für Zug (16→15→14→…→5), echte Tiefen d13–d30, König wird geführt
  (Ke5/Kf4/Kf5 statt Damen-Geschiebe), verbleibende 0-ms-Antworten sind verifizierte
  Matts innerhalb der Tiefe. KRvK-Probe (Wk1Ynq5F): mate 13 → mate 12 über Iterationen.
  Smoke grün (Buch + startpos normal). Binary md5 `c92b1241`.
- **Rollout:** Live-Bot läuft bis zum Neustart mit dem alten Binary. Kein A/B nötig
  (Bug-Fix-Kategorie wie Repetition-Fixes 02./07.05., kein Eval-Hebel) — Lichess-Lookback
  beim nächsten Fenster beobachtet missed_mate + verschenkte Remis (Erwartung: die
  6-Remis-Klasse verschwindet).

**10.06.2026 (spät) — K+P-Endspiel-Probe → ROOT CAUSE gefunden: TT-Mate-Scores ohne
Ply-Adjustment. Konversions-Bug, ≥5 verschenkte Remis im Fenster. Fix = Tobias-Entscheid.**
- **Hypothese „K+P-Technik" widerlegt:** reine K+P-Blunder nur **1/462** (und das ein
  Mate-Score-Artefakt), K+P+1-Figur 11/462 → kein Opposition/Key-Square-Cluster. Groks
  `endgame_technique`-Kategorie (168) ist identisch mit seinem Phase-Tag `endgame` —
  Korrelations-Tagging, kein Technik-Befund.
- **Echter Befund — verschenkte Gewinne:** 6 Remis im Fenster mit ≥ Turm-Vorteil am
  Partieende: **2× KRvK** (Wk1Ynq5F 50-Züge, NPiP3O5A 3-fold — eins davon in Rapid 600+5!),
  **2× KQvK** (zKfpQEn8 50-Züge in 780+5, rYuxSr81 3-fold), 1× KQ+P (p4z9XyE2 3-fold),
  1× Bullet-Mittelspiel-3-fold. clk-Beweis: Martuni verbrauchte **~50 ms/Zug** trotz
  Minuten auf der Uhr (Uhr STIEG um ~5 s/Zug) und führte in zKfpQEn8 50 Züge lang nie
  den König — nur Damen-Geschiebe.
- **Deterministischer Replay** (zKfpQEn8 ab Zug 80, echte Uhrstände, sequentiell für
  TT-Aufbau; `tools/ponder_mopup_repro.py`): Zug 82 saubere Suche **d26 „mate 12" in
  2.0 s** — ab Zug 83 nur noch **d1 in 0–9 ms**, gemeldete Matt-Distanz schrumpft NIE
  (12→14→15→15… über 65 Züge), Endstellung der Live-Partie wird mit `d64 cp 0` erreicht.
  Live-Muster exakt reproduziert. Gegenproben: frischer Engine-Start auf denselben FENs
  findet mate 5/mate 9 in <1 s und konvertiert in 12 Halbzügen (mit und ohne Ponder;
  Ponder ist unschuldig).
- **Mechanik (drei Zutaten):**
  1. `tt.store` (search.rs:859) speichert Mate-Scores **roh** — `MATE − ply` ist aber
     relativ zur Wurzel der damaligen Suche. Der Probe-Cutoff (search.rs:445) gibt sie
     unkorrigiert zurück → Matt-Distanzen sind Fossilien der alten Suche, durch die
     Negamax-Negation bleiben sie als „mate 15" stehen statt zu schrumpfen.
  2. Der **Matt-Break** (search.rs:312, `score.abs() > MATE_THRESHOLD → break`) bricht
     das Deepening schon nach **Tiefe 1** auf dem TT-Fossil ab → 0-ms-Antworten ohne
     echte Suche; der gewählte Zug liegt nicht zwingend auf einem echten Mattpfad.
  3. Der hochlaufende 50-Züge-Zähler liegt jenseits des d1-Horizonts (50-Züge-Regel ist
     in der Suche korrekt modelliert, search.rs:400, aber unsichtbar bei Tiefe 1).
  Die 07.05.-Mitigation (TT-Cutoff-Suppression bei Key in Spielhistorie) deckt nur die
  Wurzel-Keys — Kind-Knoten mit frischen Stellungen cutten ungebremst.
- **Nebenbefund (sekundär):** Mop-up-Gradient schwach — `eg_corner_weight=20` auf
  Chebyshev-Distanz zur Ecke belohnt Zentrum→Rand-Drängen mit **0 cp** (d5 und d8 sind
  beide Distanz 3 zu a8), Gesamt-Spread nur ~100 cp. NICHT die Remis-Ursache (Matt wird
  bei echter Suche trotzdem gefunden), aber bei Zeitnot ohne TT-Hilfe relevant.
  Klassische Abhilfe wäre ein Center-Distance-Term. Separat entscheiden.
- **Fix-Optionen (Engine-Logik → Tobias entscheidet):**
  - **A) Mate-Ply-Adjustment** bei Store/Probe (Standardlösung jeder TT-Engine, ~8 Zeilen
    an 2 Stellen: beim Speichern Distanz auf den Knoten normieren, beim Lesen zurückrechnen).
  - **B) Matt-Break absichern:** nur brechen, wenn das Matt innerhalb der tatsächlich
    durchsuchten Tiefe liegt (oder Break ganz streichen — er spart fast nichts).
  - **C) = A + B** (A ist die Wurzel, B billige Versicherung gegen künftige TT-Stale-Fälle).
- **Kosten im Fenster:** ≥2.5 Punkte aus 321 Partien (~6–7 Elo) nur aus den 5 klaren
  Fällen; dazu Verzerrung der missed_mate/allows_mate-Statistik. Betrifft potenziell
  jede mattnahe Stellung (auch Mittelspiel), nicht nur Endspiele.

**10.06.2026 — Lookback Damping-Rollout: Gate GRÜN, Term bleibt LIVE (50/50).**
- **Fenster:** `analyse-08.06.2026.json` = 321 Partien, ausschließlich post-Rollout
  (06.06. 19:25 UTC – 10.06. 17:46 UTC), 462 Blunder → **B/P 1.439** (von 1.542 @04.06.,
  −6.7 %). Score 58.4 %. Tagestrend B/P leicht fallend (1.47 → 1.33).
- **Rating vs Anker (06.06. Blitz 2108 / Rapid 2161):** 10.06. **Blitz 2100 (−8, Rauschen) /
  Rapid 2176 (+15)**. Der befürchtete Einbruch des global wirkenden Terms ist NICHT
  eingetreten → KPI erfüllt, `eval.toml [damping]` bleibt 50/50.
- **Profil:** `hangs_*` 0.233/P (~flat von 0.241), `allows_mate` **0.072/P** (von 0.110,
  −35 %), `missed_capture` 0.199/P (wie immer d17-Bias-verdächtig, nicht validiert),
  motivlose Drops 0.704/P = **49 % aller Blunder** (stabil, weiterhin größter Posten;
  Cluster ruht). MG 0.944/P, EG 0.396/P, Opening 0.100/P.
- **Damping-Zonen-Check:** nur 59/462 Blunder entstehen in Stellungen mit eigenem
  Materialdefizit ≥ 200 cp; dort Median-Gap (Martuni-Eval − SF-Eval) **−192 cp**, nur 7 %
  > 150 cp optimistisch (vs 13 % im Rest) — kein Optimismus-Cluster in der Zielzone.
  Schwaches Pro-Signal für die Eval-Ehrlichkeit, ohne Pre-Rollout-Vergleichswert aber
  nicht beweisend.
- **Hotspots:** stickshark99 45 P / B/P **1.98** (↑ von 1.71, Score trotzdem 61 %),
  AetherBot 66 P / 1.58 (stabil), BaymaxMate 12 P / 2.08 (neu, dünn), Boosted_Maia_1900
  11 P / 1.82; RavenEngine 11 P / Score **9 %** (schlicht stärker, kein Blunder-Ausreißer);
  sxphia 28 P / 0.86 / 89 % entschärft.
- **Grok-Analyse gesichtet (`grok_analyse_09062026.md`, eigener SF-Scan über 224 PGNs):**
  - Die Top-Liste „Auffällige Fehler in gewonnenen Partien" besteht überwiegend aus
    **Mate-Score-Artefakten** (Loss ~99 000 cp = Mate-Score-Arithmetik): langsameres Matt
    in trivial gewonnenen Q+N-vs-K- / R-vs-K-Endspielen — bekanntes Tiefenphänomen
    (vgl. missed_mate-Befund 04.05.), nicht eval-relevant. Partien wurden gewonnen.
  - **Brauchbar (a):** Endspiel-Technik-Cluster (K+P-Opposition/Key-Squares, 168 Fälle)
    deckt sich mit unserem EG-Anteil 0.396/P und dem KNP-Drift aus dem
    stickshark99-Deep-Dive → ernsthafter nächster Hebel-Kandidat (endgame.rs-Probe).
  - **Brauchbar (b):** Erweiterung des Dampings auf gegnerische Freibauer
    (enemy_passed-Optimismus, 113 Fälle) — gleiche Mechanik, denkbar als Folge-Term.
  - **Nicht verfolgen:** „King-Safety verstärken (queen_weight 6, steilere SafetyTable)"
    widerspricht unserer Empirie (King-Gefahr-A/B −29 Elo verworfen, King-Safety-Gate
    05.06. negativ). „Rook-Activity ohne Kontext" (309/536 getaggt) ist heuristisches
    Korrelations-Tagging, kein kausaler Befund — vor jedem Code-Schritt eigenständig
    verifizieren (SF-Tiefe des Grok-Scans unbekannt → d17-Snapshot-Falle).
- **Nächste Hebel-Kandidaten:** (1) K+P-Endspiel-Technik-Probe, (2) ggf.
  enemy_passed-Damping; motivlose MG-Drops bleiben diffus (kein Ein-Feature-Fix).

**06.06.2026 (spät) — Beide Hebel abgearbeitet: Damping-Term gebaut & A/B-startklar; simpleEval-Hotspot entfällt.**
- **simpleEval-Deep-Dive → kein Hebel, Hotspot entfällt.** B/P-Verlauf 2.85 (7P) → 2.08 (13P) →
  **1.93 (14P, 06.06.)**, jetzt Mittelfeld (dumbotai/Chess960/Epimetheus/Boosted_Maia liegen drüber).
  27 Blunder: 24 MG, 18 motivlos — die bekannte **diffuse motivlose MG-Drop-Signatur** (Cluster ruht
  seit 05.06.). SF-d26-Spotcheck (Be3, Martuni +440): SF +356 — beide einig „Weiß gewinnt klar" →
  161cp-„Blunder" ist d17-Rauschen ([[feedback-analyzer-d17-bias]]). Kein simpleEval-spezifischer Fix.
- **Freibauer/PST-Damping (Bxe4) → Term implementiert, A/B-startklar (NICHT gestartet).** Diagnose mit
  Produktion+SF17.1 frisch reproduziert: Blatt `…2R1K3 w` Martuni **−67** vs SF **−368** (Material
  ehrlich −223, aber `pawn_bonus +120` + `pst_eg +140` polstern); Wurzel spielt **d3e4 (Bxe4)**,
  SF-Best **d3e2 (Be2)** −118. **Neuer Term `eval::material_deficit_damping`** (eval.rs/eval_config.rs):
  liegt eine Seite statisch ≥ `deficit_threshold` cp zurück, werden ihr Freibauer-Vormarschbonus
  (`passed_pct` %) + positiver PST-eg-Überschuss (`pst_eg_pct` %) gekürzt. Integration **exakt wie
  `material_imbalance`** (eigene Funktion + Breakdown-Zeile, Konsistenz-Check grün), `evaluate_side`/
  `pawn_bonus` unangetastet. Code-Default 100/100 → **verhaltensgleich**. 82 Tests grün (1 neuer).
  - **Smoke (eval.toml `[damping]` 50/50 ab 200):** Blatt −67 → **−127** (damping −60, korrekte
    Richtung); Default-Build identisch −67; A/B-Isolation bestätigt (baseline `imbalance_8080`
    ignoriert `[damping]` → −67, Variante wertet → −127). **ABER: Wurzel kippt NICHT** auf Be2 —
    selbst bei voller Entfernung (0/0) nur −187 (vs SF −368). Die ~300cp-Optimismus stecken nicht nur
    in Freibauer+PST; ~180cp bleiben (Mobilität/Turmlinien/Materialbewertung). → **partieller
    Eval-Ehrlichkeits-Fix, kein Zug-Kipper** (wie schon beim Imbalance-Term). Wirkung ist **global**
    (jede Seite ≥200cp hinten), breiter als der enge Imbalance-Trigger → A/B misst Netto-Effekt.
  - **A/B GELAUFEN (1000 Partien, 1h46, sauber):** „Results of **baseline** vs passerdamp" →
    **passerdamp −2.08 Elo ±16.21, LOS 59.96 %** (Münzwurf), Punkte baseline 503.0 / passerdamp 497.0
    (50.30 / 49.70 %), W/L/D 415/409/176, Pentanomial [50,72,258,62,58], SPRT [0,10] unentschieden
    (LLR −0.18, leicht Richtung H0). Attribution per PGN-Zählung gegengeprüft (passerdamp 497.0,
    −6 Punkte) [[feedback-ab-attribution-check]]. **Verdikt: FLAT** — kein klarer Verlierer wie der
    Outpost (−10.77/LOS 89 %), aber auch **kein Gewinn**. Halbzeit war baseline +18 → 2. Hälfte
    aufgeholt. **Schwächerer Rollout-Kandidat als der Imbalance-Term**: der war flat im Selfplay, FIXTE
    aber sein Eval-Ziel (Nxf7-Blatt) bei engem Trigger → Lichess Rapid +80; der Damping-Term ist flat,
    **fixt sein Ziel NICHT** (Bxe4 unkippbar) und wirkt **global**. **Rollout/dormant/re-tune =
    Tobias-Entscheid** (06.06.). Live-Bot war nie betroffen (md5 `0e25cb5b…` unverändert).
  - **ROLLOUT 06.06. (Tobias): Damping LIVE, Lichess-Validierung.** `passerdamp_5050` →
    `target/release/martuni` (md5 `ca01a829…`), `eval.toml [damping]` 50/50, Bot-Neustart 21:14
    (active, „Welcome Martuni / connected", Bot war idle → keine Partie betroffen; Restart bricht
    ohnehin keine laufende Partie ab, max. ~1 s). **Anker fürs Protokoll: 06.06. Blitz 2108 /
    Rapid 2161.** Re-Check nach >300 Partien gegen die Anker; bei Einbruch Rollback =
    `eval.toml [damping]` 100/100 + Neustart (kein Rebuild) ODER Binary zurück auf imbalance_8080.
    Code noch UNCOMMITTED (Backup `martuni.imbalance_8080_20260531` bleibt). KPI: kein
    Rating-Einbruch; Idealfall Verbesserung wie beim Imbalance-Term (flat-A/B → Rapid +80).

**06.06.2026 (Abend) — Outpost-A/B VERWORFEN, zwei Hebel parallel angesetzt (Freibauer-PST-Damping + simpleEval-Deep-Dive).**
- **A/B-Ergebnis (1000 Partien, sauber beendet):** „Results of **baseline** vs outpost" →
  **outpost −10.77 Elo ±17.33**, LOS **88.89 % ZUGUNSTEN baseline**, Punkte baseline 515.5 (51.55 %),
  W/L/D 432/401/167, Pentanomial [57,64,243,63,73] (WW 73 > LL 57 für baseline), PairsRatio 1.12,
  SPRT [0,10] unentschieden (LLR 0.70). Attribution gegengeprüft gegen `run.sh`/`config.json`
  (baseline=`imbalance_8080` ignoriert die Sektion, outpost=`outpost_2515` wertet 25/15),
  [[feedback-ab-attribution-check]]. **Der Term ist hier der Verlierer.** CI deckt die Null, aber
  Richtung klar negativ — deckt sich mit dem früheren negativen Outpost-Gate (05.06.).
- **Kein Rollout.** `eval.toml [outpost]` zurück auf **0/0** (Term dormant, Code bleibt im Tree,
  verhaltensgleich). Live-Bot war nie betroffen (`target/release/martuni` == `imbalance_8080`,
  md5 `0e25cb5b…`). Backup `martuni.outpost_2515_20260606` bleibt liegen.
- **Nächste Hebel — Tobias-Entscheid: BEIDE parallel umsetzen, getrennt trackbar:**
  1. **Freibauer/PST-Damping (Bxe4-Bug)** — vertagter Präzisionsfix (Punkt b): Freibauer/PST-
     Kompensation gegen Material-Defizit dämpfen. Seltenes, *klar identifizierbares* Muster.
  2. **simpleEval-Deep-Dive** — Sparring-Hotspot-Engine, Cluster-Diagnose ab ≥15 Partien.
  Begründung der Parallelität: die beiden Fixes überschneiden sich mit sehr hoher Wahrscheinlichkeit
  null (seltenes Freibauer/Material-Muster vs. was-auch-immer der simpleEval-Fix wird) → trotz
  gleichzeitiger Umsetzung getrennt attribuierbar.

**06.06.2026 — Outpost-Standalone-A/B gestartet (Hebel 1 von 3 gewählt).** Nach Wiedereinstieg
„wo waren wir?" hat Tobias den Outpost-Term als nächsten Schritt gewählt (billigster *positiver*
Versuch, unabhängig vom ruhenden Cluster).
- **Entflechtung zuerst:** Die verworfene King-Gefahr-Änderung lag noch uncommitted in `search.rs`
  und war **nicht** dormant (kein Flag → beim Bauen aktiv) → hätte den Outpost-Build kontaminiert.
  `src/search.rs` auf HEAD revertiert; der Change als Patch `experiments/kingdanger_search_20260605.patch`
  gesichert (rekonstruierbar, nicht verloren). Binary-Backup `martuni.kingdanger_20260605` bleibt.
- **Variante gebaut + verifiziert:** `eval.toml [outpost]` 0/0 → **25/15** gesetzt. Outpost-Unit-Tests
  grün, Release gebaut. Smoke: Variante feuert auf Test-Outpost (Sd5/c4, schwarze c/e-Bauern fehlen)
  `knight_outpost=15` (EG-Phase); **Baseline `imbalance_8080` kennt die `[outpost]`-Sektion nicht**
  → ignoriert 25/15 → einziger Engine-Unterschied ist der Outpost-Term. Variante gesichert als
  `martuni.outpost_2515_20260606`, `target/release/martuni` wieder auf Produktion (md5 == imbalance_8080)
  → **Live-Bot unverändert**.
- **A/B läuft (`matches/baseline_vs_outpost/run.sh`):** SPRT [0,10], 1000 Partien, 5+0.05, UHO,
  conc=2, ~1h45, detached via nohup (PID-Start 06.06.). Invertiertes Design ggü. kingdanger: beide
  Engines lesen dieselbe Projekt-Root-`eval.toml` mit 25/15; nur die Variante wertet sie.
  **eval.toml bis Match-Ende NICHT anfassen** (wird beim Engine-Start gelesen). Danach Lichess-Lookback.
- **Challenge-Cron** weiterhin pausiert (auskommentiert) — nach dem Match ggf. wieder aktivieren.

**05.06.2026 — King-Gefahr-Suchänderung implementiert (NMP+LMR), Smoke grün, A/B bereit (noch nicht gestartet).**
Tobias-Entscheid nach der diffusen Cluster-Diagnose: **Such-Seite** (höhere ROI als diffuse
Eval-Features; Daten zeigen, dass selbst Partie-Tiefe die Angriffe verpasst). Umfang: NMP + LMR.
- **Umsetzung (`src/search.rs`):** neuer Helfer `king_in_danger(board, side)` — ≥ 2 distinkte
  gegnerische Offiziere auf die 3×3-Königszone (spiegelt `eval::king_danger`, Short-circuit bei 2).
  Pro Knoten einmal als `our_king_danger` berechnet, **gegated mit `!in_check && depth >= 3`**
  (verhaltensgleich, da NMP/LMR beide depth ≥ 3 verlangen → kein Overhead an flachen Knoten).
  Wirkt als Zusatzbedingung `&& !our_king_danger` in der NMP-Bedingung UND in LMR-`can_reduce`:
  unter Königsangriff **weniger Pruning, nie mehr**. Heftig kommentiert (Lernhilfe).
- **Smoke (05.06.):** Build clean, **81/81 Tests grün**. Quiet-Stellungen (3 Trials): NEW == OLD,
  Tiefe 7, NPS gleich (Depth-Gate eliminiert Overhead). King-Danger-Stellung: feuert (OLD d6 /
  NEW d5 bei gleicher Zeit — weniger Pruning, ~1 Ply flacher, defensiverer Zug Rd1 statt Bc2).
  Tradeoff lokalisiert auf Königsangriff-Knoten, Null-Kosten anderswo.
- **A/B bereit (Pflicht vor Rollout — Such-Änderungen historisch heikel):** Skript
  `matches/baseline_vs_kingdanger/run.sh` (SPRT [0,10], 1000 Partien, 5+0.05, UHO, conc=2,
  ~1h45). baseline = `martuni.imbalance_8080_20260531`, variant = `martuni.kingdanger_20260605`.
  Beide dieselbe eval.toml (Outpost dormant → isoliert die Such-Änderung). **Noch NICHT gestartet**
  — läuft auf der Live-Bot-Maschine, Tobias entscheidet Start. Danach Lichess-Lookback.
- **Bot unverändert / Sicherheit:** `target/release/martuni` wurde nach dem Build wieder auf die
  Produktion (`martuni.imbalance_8080_20260531`) zurückgesetzt — ein Bot-Neustart lädt also die
  geprüfte Version, NICHT die ungetestete King-Gefahr-Änderung. **Rollout NACH grünem A/B+Lichess:**
  `cp target/release/martuni.kingdanger_20260605 target/release/martuni` + Bot-Neustart.
- **A/B-ERGEBNIS 05.06. (1000 Partien): King-Gefahr VERLIERT — verworfen.** „Results of **baseline**
  vs kingdanger": **Elo +28.90 ±17.30 für baseline** (Produktion), kingdanger 45.85 % / 458-375-167,
  LLR 2.55, CI schließt 0 aus → signifikant. Attribution gegengeprüft per PGN-Punktezählung
  (baseline 541.5 vs kingdanger 458.5), [[feedback-ab-attribution-check]]-Lehre beachtet.
  **Diagnose des Fehlschlags:** der `king_in_danger`-Trigger (≥2 Angreifer auf die Zone) feuert im
  normalen Mittelspiel viel öfter als *echte* Angriffe → NMP/LMR breit aus → Tiefenverlust überall
  kostet mehr als das Erkennen seltener Angriffe bringt. **Kein Rollout**, Bot war nie betroffen
  (`target/release/martuni` = Produktion). King-Gefahr-Code bleibt als dokumentiertes Negativ-
  Experiment im Tree (dormant, da depth-gegated + nur bei ≥2 Angreifern). Bestätigt: auch die
  Such-Seite hat im diffusen Cluster keinen leichten Gewinn.
- **Option 4 — Selektions-Bias gemessen (05.06., `tools/selection_bias.py`):** 60 zufällige
  Martuni-MG-Züge mit `%eval`, SF d26 before+after. **Nicht-Blunder-Gap median +97 cp** (mean +88,
  Perzentile 10/25/50/75/90 = −98/−12/+97/+203/+288), MG-Blunderrate 5/60 (8 %). Cluster-Blunder
  hatten +250 → **cluster-spezifischer Extra-Optimismus ~+153 cp über dem Baseline**.
  - **Befund:** Der +97-Baseline ist größtenteils **inhärent** (Optimizer's Curse — gemessen wird
    Martunis Eval genau des selbst-gewählten, höchstbewerteten Zuges → systematisch optimistischer
    als neutraler Tiefen-Eval; plus Tiefen-/Stärke-Abstand flache Blitz-Suche vs SF d26), **kein
    fixbarer Bug**. Die +153 obendrauf bei Blundern sind der echte cluster-spezifische Anteil.
  - **Schluss:** ~40 % des scheinbaren +250-Cluster-Gaps war inhärentes Mess-Bias. Der echte Rest
    ist diffus und ohne sauberen Hebel (Mobility/King-Safety/Outpost ausgeschlossen, Such-King-Gefahr
    A/B-verloren). **→ Motivlosen Cluster ruhen lassen** (Roadmap-Option 3); kein weiteres Feature
    blind darauf bauen. Nächste Hebel: simpleEval-Deep-Dive (≥15 Partien) oder vertagtes
    Freibauer/PST-Damping. Outpost-Term bleibt dormant als eigenständiger A/B-Kandidat.

**05.06.2026 — Cluster statistisch ausgezählt (222 MG-motivlose): diffus, KEIN Ein-Feature-Fix.**
Größere Stichprobe (Tobias-Entscheid „erst messen"): alle 222 MG-motivlosen Stellungen bei SF
verifiziert + aktionables Motiv ausgezählt (`tools/classify_cluster.py` / `classify_report.py`).
- **Cluster real:** 82 % überleben (181/222), davon 172 echt positionell, 9 Tiefen-Taktik (Suche).
  Median Optimismus-Gap **+250 cp** (109/146 > 150 cp) — robust über alle Buckets.
- **Kein dominantes Motiv** (Prioritäts-Klassifikation der 172): `other_passive` 40 % /
  `pawn_break` 23 % / `king_danger` 21 % / `greedy_passive` 16 %. Höchste Gaps (= Eval am
  falschesten): **king_danger 346 cp**, **greedy_passive 343 cp**; `pawn_break` nur 149 cp.
  SF-Bestzug: 62 % Figurenzug, 30 % Bauernhebel, 8 % Königszug.
- **King-Safety-Vorabcheck (Lehre aus Outpost):** In den 42 king_danger-Stellungen feuert
  Martunis bestehender king_safety **median nur −10 cp** (14/42 sehen GAR KEINE Gefahr) —
  er unterfeuert real. ABER: selbst ein korrekt feuernder Term (typ. 20–80 cp) schließt keinen
  346-cp-Gap; der Rest ist gegnerischer Angriff, der über Taktik landet (= Such-Seite).
- **Schlussfolgerung:** Die motivlosen Drops sind **kein einzelner adressierbarer Hotspot**,
  sondern der diffuse Long-Tail von „Eval ~250 cp zu optimistisch in ruhigen Stellungen" —
  Mischung aus Selektions-Bias (per Definition Martunis Eval-Fehlmomente), echten verteilten
  Positionswissens-Lücken UND Such-/Horizon-Anteil. Kein eval-Feature deckt > 40 %, und die
  größten Slices haben Gaps, die ein statischer Term nicht schließt.
- **Optionen (Tobias entscheidet):** (1) **Such-Seite King-Gefahr** (Extensions / weniger LMR
  wenn eigener König angegriffen) — die Daten stützen, dass selbst Partie-Tiefe die Angriffe
  verpasst → evtl. höhere ROI als Eval; (2) **King-Safety strukturell nachschärfen** (unterfeuert
  belegt) + Outpost-Term standalone A/B — kleine, sichere Eval-Gewinne, aber je < 21 % Coverage;
  (3) Cluster als „kein klarer Hebel" akzeptieren und zu anderem Roadmap-Punkt wechseln.

**05.06.2026 — Knight-Outpost-Term implementiert (Code-Default 0), ABER Diagnose-Gate negativ für den Cluster.**
Tobias wählte für die motivlosen Drops die Eval-Seite → Aktivität/Outposts → „Springer, flach,
gedeckt". Umgesetzt mit dem etablierten sicheren Muster: `eval::is_knight_outpost` (Springer
4.–6. Reihe, bauern-gedeckt, kein gegn. Bauern-Angriff möglich = „Loch"), phase-getapert, Hook in
`evaluate_side` + Breakdown (`knight_outpost`-Zeile), Parameter `eval.toml [outpost]`
(`knight_mg`/`knight_eg`), **Code-Default 0 → verhaltensgleich**. Build clean, **81/81 Tests grün**
(2 neue). `eval.toml` trägt die Sektion mit 0/0 (inaktiv) — kein Live-Effekt bis Rollout-Entscheid.
- **Gate-Ergebnis (negativ):** Auf den 35 Diagnose-Stellungen feuert der Term **nur in 1/35**
  (beim Gegner), Optimismus-Gap **unverändert +309 → +308 cp** (`tools/outpost_probe.py`).
  → **Outposts kommen in diesem Cluster praktisch nicht vor**; der Term repariert die motivlosen
  Drops NICHT. Der Term selbst ist korrekt und sinnvoll — aber als **eigenständiger
  Eval-Verbesserungs-Kandidat** zu sehen, nicht als Cluster-Fix.
- **Prozess-Lehre:** Feature-Prävalenz im Ziel-Cluster VOR dem Bauen prüfen (hätte den
  Fehlschuss gespart). Die 6 qualitativen Survivors zeigen als wiederkehrendes Eval-Motiv eher
  **Schwerfiguren-Passivität + verpasste Bauernhebel (…f5/…g5) + zugige eigene Königsstellung**,
  NICHT Springer-Outposts.
- **Status Outpost-Term:** implementiert, dormant (0/0), getestet → **bereit für eigenständigen
  A/B**, falls gewünscht; ODER shelven. Cluster-Fix offen (Richtung: Schwerfiguren-Aktivität /
  Bauernhebel-Bewusstsein — vor dem Bauen erst größere Stichprobe statistisch auszählen).

**05.06.2026 — Lookback Imbalance-Term (80/80) grün: Gate passed, B/P flat, Zielmuster aber selten.**
Auswertung `analyse-04.06.2026.json` (**382 Partien, 589 Blunders, B/P 1.542**
— flat ggü. 1.54 @01.06.; Trendserie 1.74 → 1.589 → 1.54 → 1.542 stabilisiert).
Verteilung: Mittelspiel 411 (1.076/P) / Endspiel 147 (0.385/P) / Eröffnung 31
(0.081/P). Das gesamte Fenster lief **mit aktivem Imbalance-Term** — laufendes
`target/release/martuni` ist per Hardlink identisch mit dem Backup
`martuni.imbalance_8080_20260531`, `eval.toml [imbalance]` = 80/80.

- **Rating-Gate grün (KPI „kein Rating-Einbruch vs Anker" erfüllt):** Anker 31.05.
  Blitz 2101 / Rapid 2118 / Bullet 2135. Live-Stand 05.06. (Lichess-API, alle
  nicht-provisorisch): **Blitz 2095 (−6, Rauschen)**, **Rapid 2198 (+80, klar
  positiv)**, Bullet 2094 (−41). Kein Einbruch in Blitz/Rapid, Rapid deutlich hoch.
- **Zielmuster (Nxf7/Bxe4 „2 Leichtfiguren vs Turm+Bauer") kam live kaum vor:**
  Die Imbalance-Signatur (eine Seite ≥2 Minor mehr UND ≥1 Turm weniger) tritt in
  nur **26/589** Blunder-Stellungen auf, davon genau **1** mit `trade_down`-Motiv.
  Die `trade_down`-Blunder (27, 0.071/P) sind **nicht** das Imbalance-Muster,
  sondern schlichte Hänger (`hangs_bishop/knight/queen`) + Dame-ins-Matt — gleiche
  Minor-Zahl auf beiden Seiten. → Der Term ist als **harmlos + eval-ehrlich**
  validiert (Gate passed), aber die Live-Daten konnten die anvisierte Pathologie
  kaum exerzieren. Bestätigt die 31.05.-Prognose: „ein Einzel-Term ist nur ein
  Teilfix; die zwei Eval-Lücken sind in der Suche verschränkt".
- **`hangs_*` leicht runter** (92, 0.241/P vs 0.260/P @01.06.), `allows_mate`
  stabil (42, 0.110/P — wie 0.109 @09.05., keine Regression). Auf den 26
  Signatur-Stellungen ist Martuni weiterhin median ~220 cp zu optimistisch
  (13/22 >150 cp), aber **d17-Snapshot-Trap-Vorbehalt** gilt (Analyzer-Tiefe,
  heterogene Stichprobe, nur 1 `trade_down`) → kein Deep-Dive-würdiger Cluster.
- **Strukturprofil unverändert:** 295/589 (50 %) **ohne Motiv** (leise positionelle
  Drops); `missed_capture` 70 (median loss nur 244 cp, alle Captures) — konsistent
  mit Analyzer-d17-Bias, **nicht** als real validiert; `positional_collapse` 68 /
  `exposed_king` 33 großteils in bereits verlorenen Stellungen; Endspiel-Matt-Klasse
  29 (KP/Q-Endspiel-Tiefentaktik, Such- nicht Eval-Problem).
- **Hotspots:** stickshark99 (58 Partien, B/P 1.71 — großes Sample, persistent),
  AetherBot (53, 1.62 — stabil ggü. 1.65 @23.05.), EpimetheusBot (22, 1.91 —
  hat bereits Buch-Patches), Boosted_Maia_1900 (14, 2.14 — neuer, beobachten).
  **simpleEval** (Re-Check-Ziel der Roadmap): 13 Partien, **B/P 2.08** (von 2.85
  / 7 Partien runter, Trend↓), Schwelle ≥15 Partien fast erreicht — weiter tracken.
- **Empfehlung/offen:** Imbalance-Term **behalten** (Gate grün, kein Risiko).
  Freibauer/PST-Damping (Bxe4-Bug, b) wird **auf später vertagt** (Tobias 05.06.):
  Imbalance-Muster live selten, kein Passer-Cluster sichtbar → geringe Dringlichkeit.
  **Gewählter Hebel: die 50 % motivlosen positionellen Drops** (siehe Diagnose unten).

**05.06.2026 — Diagnose „motivlose Drops": echter Eval-Optimismus-Cluster, kein d17-Noise.**
Die 295/589 motivlosen Blunder (kein taktisches Motiv, alle loss 150–296 cp — per
Konstruktion der ruhige Rest-Bucket unterhalb der `positional_collapse`-Schwelle 300)
tief verifiziert: Stichprobe 50 (geschichtet Phase × Verlust) bei **SF movetime 2.0 s
(~d26)** statt der Analyzer-0.3 s, `before+after` neu bewertet (`tools/verify_nomotif.py`).
- **Cluster ist real, kein Snapshot-Noise:** **78 % überleben** (deep_loss ≥ 150),
  nur 10 % verpuffen. **Phasenabhängig:** Mittelspiel **90 %** real (median deep_loss
  198), Endspiel nur **36 %** (median 50 — der Endspiel-Anteil ist 0.3-s-Noise, da SF
  flach im EG schwach; **Endspiel-Motivlos depriorisieren**).
- **Zwei Hebel in den Survivors:** ~10 % **Tiefen-Taktik/Matt** (Martuni + flacher
  Analyzer übersehen erzwungenes Matt → `search.rs` Extensions/Tiefe, kleinerer
  Cluster); ~90 % **echte positionelle Drops** (150–500 cp) → Eval.
- **Root-Cause (Eval-Breakdown-Aggregat über 35 positionelle Survivors,
  `tools/eval_breakdown_agg.py`):** Martunis Eval ist **konsistent ~300 cp zu
  optimistisch** — Game-Depth-`%eval` median **+283 cp** zu rosig (24/27 >150 cp),
  statische Eval **+309 cp** (Martuni Ø +134 vs d26 Ø −175). Attribution: Optimismus
  dominiert von **`material` (+62.5 cp Mover-Sicht)** in objektiv verlorenen Stellungen,
  während die **dynamischen/positionellen Terme zu dünn** sind: `mobility_mg` **+0.3**,
  `mobility_eg` +3.7, `king_safety` +5.7. SF sieht ~300 cp Kompensation/Dynamik, für die
  Martunis Eval **keinen ausreichenden Term hat**.
- **Befund:** Dieselbe Familie wie Nxf7/Bxe4 (Material > Kompensation), aber **allgemein**
  statt schmales Imbalance-Muster. Strukturell: Eval ist bewusst materialistisch mit
  gedeckelter Safe-Mobility (Summe ~100–120 cp) und **fehlendem Positionswissen**
  (rückständige Bauern / schwache Felder / Outposts laut [blunder-analyse.md](blunder-analyse.md)
  Z.137 nicht vorhanden). Passt zum simpleEval-Hotspot (Engine-Gegner bestrafen Materialismus).
- **Probe-Ergebnis 05.06. — beide vorhandenen Gewichts-Hebel ausgeschlossen.** Tobias wählte
  „Mobility-Probe zuerst". Zerstörungsfrei getestet (Test-`eval.toml` in Temp-CWD,
  `tools/mobility_probe.py` / `tools/ksafe_probe.py`), Gate = Optimismus-Gap auf den 35
  Diagnose-Stellungen (Baseline +309 cp):
  - **Mobility (Minoren+Türme MG ×2): Gap +309 → +288 cp** (nur ~7 % geschlossen; selbst
    Verdopplung verschiebt die Eval ~5 cp/Stellung — der Mobility-DIFF zwischen den Seiten
    ist strukturell zu klein). **Ausgeschlossen.**
  - **King-Safety (Angreifer-Gewichte ×2 + SafetyTable ×1.5): Gap +309 → +297 cp** (~4 %).
    **Ausgeschlossen.**
  - **Schluss:** Der fehlende ~300 cp wohnt in **keinem existierenden Eval-Term** — kein
    billiger TOML-Fix. Es ist *strukturell fehlendes* Positionswissen (das, was Safe-Mobility
    laut Doku ignoriert: Pins, Koordination, Initiative, Bauernschwächen, Outposts; vgl.
    [blunder-analyse.md](blunder-analyse.md) Z.137). Methodik-Lehre: existierende
    eval.toml-Gewichte VOR jedem A/B billig am Diagnose-Set gaten — spart Selfplay/Lichess-Zyklen.
- **Nächster Schritt (offen):** Da Gewichts-Tuning raus ist, bleibt (2) **neues Positions-
  wissen in `eval.rs`** (Outposts / rückständige Bauern / schwache Felder) — echte Eigenarbeit.
  Empfehlung: erst **qualitative Stellungs-Diagnose** (Handvoll Survivors, SF-Plan vs
  Martuni-Zug) um den **EINEN dominanten fehlenden Begriff** zu finden, statt alle Features
  gleichzeitig zu bauen. Eval-Änderungen historisch regressionsanfällig (conn_rooks=150,
  Passer-Raise) → **immer A/B + Lichess vor Rollout**.

**31.05.2026 — Auswertung 01.06., Lookback grün, SEE-Follow-up als Eval-Problem entlarvt.**
Auswertung `analyse-01.06.2026.json` (173 Partien, 266 Blunders, **B/P 1.54**
— Trend runter: 1.74 @27.05. → 1.589 @29.05. → 1.54). Verteilung: Mittelspiel
172 / Endspiel 79 / Eröffnung 15; 18 Mate-Blunder; 45× `hangs_*`. Achtung:
gemeinsames Lookback-Fenster für **Freibauer-Revert (9bf3d11) UND SEE-Pruning
(765e727)** ab Anker Blitz 2040 / Rapid 2083 — B/P-Verbesserung nicht einem
Einzel-Change zuschreiben.

- **Rating-Gate grün:** Lichess 31.05. **Blitz 2101 (+61)** / **Rapid 2118
  (+35)** / Bullet 2135, beide nicht-provisorisch. Das gemeinsame Fenster ist
  klar positiv (keine Regression).
- **King-Safety-Lead verworfen:** Roh sahen 47 % der echten Blunder „blind"
  (Martuni-Eval >150 cp zu rosig), Top-Motive positional_collapse/exposed_king.
  Verifikation der Top-8-Cluster bei **SF d23–26** zeigt: kontaminiert (nur 3/8
  echtes Mittelspiel) und die 3 echten Fälle bestätigen die Eval-Blindheit
  **nicht** (Martuni-Eval real in Ordnung). Klassischer d17-Snapshot-Trap →
  King-Safety ist aus diesem Batch **kein** validierter Hebel.
- **SEE-Follow-up (depth ≤ 3) empirisch erledigt:** Die in [see.md](see.md)
  skizzierte schärfere Stufe wurde reproduktions-getestet, bevor ein A/B läuft.
  Ergebnis: depth ≤ 3 (tiefenskaliert −100 @d3) **und** depth ≤ 4 flach
  **kippen Nxf7/Bxe4 NICHT** — der Nxf7-Eval ist invariant bei −25 cp gegen die
  Pruning-Tiefe. Die Kompensation wird nicht von verlierenden Captures getragen
  → **kein Such-Pruning-Problem.** Committed depth ≤ 2 (765e727) bleibt
  unverändert. (Methodik-Lehre: zeitbasiert testen — `go movetime` + stdin via
  `sleep` offen halten; `go depth N` + sofort `quit` bricht die Suche bei
  depth ~6 ab. Gilt auch für Stockfish-Pipes.)
- **Root-Cause-Diagnose (UCI `eval`-Breakdown + python-chess):** Die zwei Opfer
  sind **zwei verschiedene Eval-Baustellen** mit gleichem Symptom (~290 cp zu
  optimistisch beim Tausch Material-für-Kompensation):
  - **Nxf7** = **Material-Imbalance.** Forcierte Linie Nxf7 Rxf7 Bxf7+ Qxf7 →
    Weiß gibt Springer+Läufer für Turm+Bauer. End-Blatt: Martuni statisch
    **−35** vs SF **−327**; `material`-Diff nur **−15** → Martuni bewertet
    R+P ≈ N+B, das Wissen „zwei aktive Leichtfiguren > R+P im MG" fehlt.
  - **Bxe4** = **überbewertete Freibauer/PST-Kompensation.** Linie Bxe4 Qxe4 …
    d5×c6 …; Illusions-Blatt: Martuni **−67** vs SF **−360**; `material`
    ehrlich −223, aber `pawn_bonus`-Diff **+120** + `pst_eg`-Diff **+140**
    (vorgeschobener c6-Passer) erkaufen die fehlende Leichtfigur scheinbar zurück.
- **Maßnahme (a) umgesetzt — Imbalance-Term „2 Leichtfiguren vs Turm(+Bauer)".**
  Neuer Eval-Term in `eval::material_imbalance` (Hook in `evaluate()` + im
  `eval`-Breakdown sichtbar): löst nur beim klassischen Muster aus (eine Seite
  ≥ 2 Leichtfiguren mehr UND ≥ 1 Turm weniger), Bonus für die Minor-Mehrheits-
  Seite, phase-getapert. Parameter in `eval.toml [imbalance]`
  (`two_minors_mg`/`two_minors_eg`), **Code-Default 0** → ohne die Sektion
  verhaltensgleich zur Vorversion (sauberer Toggle, Rollback ohne altes Binary).
  - **Smoke-Befund (wichtig):** Der Term repariert die *Bewertung* (Nxf7-End-
    Blatt: alt −35 / mit Term −115, näher an SF −327), **kippt die Zugwahl bei
    vernünftigen Werten aber NICHT** — die Suche entkommt per Damentausch ins
    Endspiel und weicht in die zweite, noch ungedämpfte Baustelle aus
    (Freibauer/PST-Überbewertung, der Bxe4-Bug). Nxf7 kippt erst bei flat ~150
    (global riskant, Overfit auf eine Stellung). Lehre: die zwei Eval-Lücken
    sind in der Suche verschränkt; ein Einzel-Term ist nur ein Teilfix.
  - **Rollout-Entscheidung (Tobias):** **flat MG 80 / EG 80** ausgerollt — macht
    die Eval ehrlich ohne Overfit; kein fastchess-A/B, sondern **> 300 Lichess-
    Partien als Richter**. Lookback-Anker = **Blitz 2101 / Rapid 2118 (31.05.)**.
    Build clean, 79/79 Tests grün, Backup-Binary
    `target/release/martuni.imbalance_8080_20260531`. **Rollback** = `[imbalance]`
    in eval.toml auf 0 (oder Sektion löschen) + Bot-Neustart, kein Rebuild nötig.
    Aktivierung: `cargo build --release` (erledigt) + Bot-Neustart (lädt neues
    Binary + eval.toml neu). KPI: Nxf7/Bxe4-artige `trade_down`+`hangs_*`-Cluster
    sinkend, kein Rating-Einbruch vs Anker.
- **Offen:** (b) Freibauer/PST-Kompensation gegen Material-Defizit dämpfen
  (Bxe4-Bug) — wegen der Verschränkung evtl. nötig, damit (a) den Zug auch
  wirklich kippt. simpleEval (B/P 2.85 / nur 7 Partien) nach ≥ 15 Partien
  re-checken. Diagnose-Detail: [see.md](see.md) Root-Cause-Abschnitt.

**29.05.2026 — Freibauer-Regression entdeckt + revertiert (9bf3d11).**
Auswertung `analyse-29.05.2026.json` (129 Partien, 205 Blunders,
B/P **1.589** — Regression ggü. 1.32 vom 20.05.). Rating-Abfall seit
23.05.-Anker: Blitz 2066→**2040**, Rapid 2123→**2083**. Ursache
gefunden: Commit `da625d4` „Freibauer-Bonus angehoben
[5,15,35,70,150,300]" hat den **A/B-Verlierer** committet. fastchess
(`matches/gate700_vs_passed_pawn_v1`, 1000 Partien): *„Results of
Baseline vs PassedPawnV1 … Elo: 20.17 ±17.69, LOS 98.75 %, Wins 463 /
Losses 405"* → die **+20 Elo gehören Baseline** (alte Werte
[5,15,30,55,100,170]), nicht dem Raise. Belegt über `variant/eval.toml`
(neue Werte) vs `baseline/eval.toml` (Key fehlt → Code-Default).
**Revert `9bf3d11`** zurück auf [5,15,30,55,100,170], Bot 13:43 neu
gestartet (eval.toml ist Laufzeit-Config, kein Rebuild nötig).
Erwartete ~+20-Elo-Erholung — Lookback-Anker **Blitz 2040 / Rapid 2083
(pre-revert)**. Methodik-Lehre: vor dem Committen eines A/B-„Siegers"
prüfen, welcher Engine das +Elo gehört (Win-Count vs variant-eval.toml).
pQ8INwpS-7th-rank-Motivation (170 cp evtl. zu niedrig) bleibt offen für
einen *milderen*, sauber getesteten Bump.

**Blunder-Cluster 29.05. (nächster Hebel = SEE):** 38 hängende Figuren
(Springer 16, Läufer 14, Turm 6, Dame 2) plus Muster „eigener Schlagzug
verliert Material" (`Nxf7`/`Nxe5`/`Bxe4`/`Nxd5`, jeweils `hangs_* +
trade_down`). 63 echte Selbst-Delusionen (Martuni-Eval rosig, Realität
bricht ein, kein d17-Bias). Befund: SEE wirkte überall *außer* in der
Hauptsuche — verlierende Captures (SEE<0) wurden dort voll durchsucht.
**Konservatives SEE-Pruning in `alpha_beta` ausgerollt** (Commit
`765e727`, Bot 16:48 neu gestartet): `!is_pv && !in_check &&
!child_in_check && depth<=2 && move_idx>0 && see<0 → continue`, nutzt
gecachten `sm.see_val` (gratis). A/B `matches/baseline_vs_see_prune_v1/`:
**+12.17 Elo ±18.39, LOS 90.29 %** (1000 Partien), Detail in
[see.md](see.md). Lichess-Lookback entscheidet; Rollback =
`git revert 765e727`. Entschärfungen halten: stickshark99 2.16→1.20,
sxphia 1.42→1.25.

**23.05.2026 15:33 — Pawn-Endgame-Guard ausgerollt, tt.rs aufgeräumt.**
Drei Sub-Konzepte (Opposition direkt+diagonal, Key Squares per Rang,
Rook-Pawn-Edge) hinter hartem NPM-Gate ≤ 700 cp / Phase-Tapering.
A/B-Match (1000 Partien 5+0.05 UHO): **-2.43 ±15.94 Elo für Guard**,
LOS 38.2 %, LLR -0.14 (SPRT nicht terminiert). Pgn-Drilldown: nur
18 % der Spiele enden im Term-aktiven Bereich (NPM ≤ 700), dort
50.5 % Score — Selfplay-Spiegelstil neutralisiert die enge
Subgruppe. Trotzdem ausgerollt analog Step 2 v2 / Step 3
([[feedback-ab-vs-lichess-signal]]): Bot-Mix auf Lichess hat andere
EG-Dichte, gerade fuer die KNP-Cluster aus
[[project-stickshark99-deepdive-2026-05-23]] (`MH3BeAfV` ply 91 wird
mit Guard zu Kc4 statt Kd4 — direkte Bestätigung der Hebel-Mechanik).
Lookback-Anker 23.05.2026: **Blitz 2066 / Rapid 2123**. KPIs:
stickshark99 B/P 2.16 → ≤ 1.5, `positional_collapse` im Endspiel
stabil/sinkend, kein Anstieg `hangs_pawn`/`exposed_king`.
Konzept-Doku: [pawn-endgame-guard.md](pawn-endgame-guard.md),
Implementierung: [[project-pawn-endgame-guard]],
Match: `matches/baseline_vs_pawn_eg_guard/`.

**Parallel-Aufräumen 23.05.2026:** Audit über bekannte tote Stellen
gefahren. Aktionen: `tt.rs` Phase-1-Doc-Kommentar neu gefasst,
8 `#[allow(dead_code)]`-Annotations entfernt (TtFlag/TtEntry/probe/
store waren lange aktiv), `capacity()` als tatsächlich tot entfernt;
`passed_bonus = 300` aus `eval.toml [pawns]` gelöscht (Konfigurations-
Drift, Loader las den Key nie). Build clean, 79/79 Tests grün.
Stehende Audit-Punkte (Tobias-Entscheidung pendend): `bishop_pair_each`
als Anker behalten oder weg, `pub fn taper` / `pub fn evaluate_breakdown`
auf privat reduzieren, `is_passed_simple` vs `is_passed` konsolidieren
(letzteres steht schon unten in „Offene Themen").

**20.05.2026 21:15 — Step-3-Lookback bestätigt, neuer Engine-Befund.**
291 Partien post-Rollout: Lichess Blitz **2039 → 2080** (+41), Rapid
**2100 → 2142** (+42). Blunder/Partie 1.39 → 1.32, keine
Regression. Step 3 bleibt — Backup-Datei
`target/release/martuni.backup-pre-step3-20260516` kann entfernt
werden. Volle Auswertung: `analyse-18.05.2026.json` /
[[project-auswertung-2026-05-20]].

**Operative Änderung 20.05. ~22:08:** `lichess-bot/config.yml`
`challenge.concurrency` **3 → 2** reduziert. Begründung: VM hat
2 logische Kerne; 3 parallele Engines kosteten in Stresstest ~33 %
NPS / ~0.5–1 Tiefe pro Zug. Hard-Restart durchgeführt (idle).

**21.05.2026 — m19/PVS-Recheck:** Die Vermutung "Off-by-one im
Root-PVS-Scout" ist nach Reproduktion **nicht bestätigt**. Das
Nullfenster im Root-Loop steht korrekt auf `(-alpha - 1, -alpha)`, und
`score == alpha` bei Folgezügen ist fuer fail-hard/Nullfenster-PVS ein
normaler Bound, kein exakter Zugwert. Die angeblich objektiv gewinnende
Probe `d1d7` ist in der Stellung
`4kb1r/rqp2ppp/1p2P3/8/8/2Q5/PP3PPP/R2R2K1 w k - 0 19` kein klarer
Gewinn: aus der Folgestellung findet Martuni bei d7 unter anderem
`f7e6`, Bewertung auf Tiefe 7 ca. ausgeglichen. Root-Suche mit
`MARTUNI_NMP_OFF=1`, Hash=1 MB und `go depth 8 movetime 100000` bleibt
bei `c3c4` mit `+169 cp`. Aktuell daher **kein PVS-Fix offen**; bevor
Search-Code geaendert wird, braucht es eine Gegenprobe mit einem
Root-Folgezug, der in voller Suche eindeutig `> alpha` ist, im
PVS-Scout aber trotzdem bei `score == alpha` haengen bleibt.

**Methodischer Hinweis — Analyzer-Tiefen-Bias.** Drill auf 9 Stichproben
der 18.05.-Auswertung zeigt: mind. 4/6 `missed_capture`-Befunde sind
reiner SF-d17-vs-Engine-d8-Tiefenbias, **kein Engine-Defekt**. Vor
Eval/Search-Triggern künftig Cross-Check mit Martunis eigener Tiefe
machen, siehe [[feedback-analyzer-d17-bias]].

---

**Dynmat-Step3 (Bishop-Pair-Tapering Variante A) am 16.05.2026 ~22:23
nach 1000-Partien-A/B ausgerollt.** A/B-Match
`matches/baseline_vs_dynmat_step3`: Step3 nominell **+6.95 Elo ±18.06**
gegen pre-Step3-Baseline, LOS 77.5 %, 411W / 158D / 431L (Baseline-
Sicht), DrawRatio 44.8 %, PairsRatio 0.85. SPRT [0, 10] hat nicht
terminiert (LLR −1.10, Match komplett durchgespielt 1000/1000) — Profil
sehr ähnlich zu Step 2 v2 (+5.56 Elo, LOS 72 %). Werte in eval.toml:
`bishop_pair_mg = 30`, `bishop_pair_eg = 50`, `bp_open_scale = 2`.
Sanity-Check im Eval-Breakdown verifiziert (MG/Phase 23 → 30 cp = alter
Wert; EG/Phase 2 → 68 cp). Backup als
`target/release/martuni.backup-pre-step3-20260516`, Lookback offen.
**Rollout-Entscheidung trotz CI-deckt-Null**: Step 2 v2 hatte unter
identischer Datenlage auf Lichess flat (kein Rückschritt) abgeschnitten,
und der Selfplay-↔-Lichess-Caveat aus
[[feedback-ab-vs-lichess-signal]] gilt für Eval-Material-Hebel weiterhin
— Spiegelstil im A/B ist nicht repräsentativ für Bot-Mix.

Lichess-Stand 16.05. (vor Rollout, Cluster-1b/CR30-Effekt): Blitz
**2039** (+29 ggü 15.05.), Rapid **2100** (+17). Nächster Lookback in
100–150 Partien gegen diesen Anker.

Vorgeschichte: Step 2 v2 wurde am 13.05. trotz nicht-signifikantem A/B
ausgerollt, am 15.05. nach 144 Partien Lichess-Lookback behalten
(Blitz 2027 → 2010, Rapid 2075 → 2083, netto flach im Rauschen).
`connected_rooks_pair = 150 → 30` am 15.05. live nach SPRT-Sieg
+150.65 Elo ±41 (240 Partien, H0). Cluster-1b-Stichprobe ergab **2 echte
missed_capture-Bugs** (`54iwUiMx`, `W5AboGf0`); Aspiration Windows und
Centipawn-MVV wurden am 16.05. als Lösungsversuche **verworfen** (siehe
Punkte 2/3 unten).

**Befunde aus dem 15.05-Lookback und Cluster-1-Stichprobe:**
- Vier Buch-Patches für `stickshark99` ausgerollt (15.05.,
  `martuni_patches.bin` jetzt 6 Einträge).
- Cluster-1-Stichprobe `missed_capture` (`tools/probe_missed_captures.py`):
  3/9 lösen sich mit Tiefe (Such-/NPS-Hebel), 3/9 sind echte Eval-Hebel
  in der Erstbewertung — nach SF-Sanity-Check bis d30
  (`tools/verify_cluster1b_stockfish.py`) bleiben **2/9 als echte
  Bugs**: `54iwUiMx` m25 (Nxd4) und `W5AboGf0` m29 (Qxd6).
  `m1oQlfmG` war SF-Artefakt der d17-Momentaufnahme (Stockfish d30
  selbst flippt auf `Be2`).
- **W5AboGf0 Deep-Dive** (`tools/trace_w5abogf0.py`): Engine bei
  120 s/100 M Knoten erreicht d10 und bleibt bei Qg4 (−49 cp).
  Post-Move-Eval an d10 separat: Qxd6 = **+47 cp**, Qg4 = **−44 cp**
  (Diff 91 cp). SEE rechnet die Q-für-Q-Capture korrekt mit Netto 0.
  Diagnose: **Such-Tiefe-/Move-Ordering-Issue, kein statischer
  Eval-Bug**. Bei Root-d11 wäre der Flip da, kostet aber ~5–10× mehr
  Knoten als das Blitz-Budget liefert.
- Neue Hotspots: `stickshark99` 2.25 B/P (vor den Patches),
  `AetherBot` 2.04 B/P (25 Spiele) — eigener Lookback offen;
  `sxphia` von 1.7 auf 1.42 B/P gefallen.

## Nächste Schritte

1. **AetherBot-Lookback** als nächste Eval-Priorität, nachdem Schritt 3
   der dynamischen Figurenbewertung am 16.05. live ist (siehe „Aktueller
   Status" und Verlauf). Im 16.05.-Sample 3.38 B/P bei 8 Spielen
   ([[project-aetherbot-lookback-2026-05-16]]) — bisher zu dünn für
   einen Buch-Patch, aber das deutlichste neue Sparring-Signal.
   Erst-Re-Check empfohlen nach ≥20 Partien. Parallel dazu wartet die
   Step-3-Lichess-Lookback-Erfassung (100–150 Partien gegen Blitz 2039 /
   Rapid 2100).
2. **Aspiration Windows — VERWORFEN am 16.05.2026.** Variante B
   (δ=30 cp, Faktor 2, ab d≥5) implementiert und auf den 9 Cluster-1b-
   Stichproben gemessen: Σ maxD −2, Re-Search-Quote **102 %**, und
   W5AboGf0 spielt mit Aspiration den falschen Zug (`Qg4` statt
   `Qxd6`), den die volle Suche bei gleicher Tiefe findet. Cluster-1b-
   Stellungen sind genau Score-Diskontinuitäten zwischen ID-Tiefen
   (~90 cp Sprünge); ±30 cp Startfenster ist viel zu eng, exponentielles
   Widening verbrennt mehr Knoten als das engere Fenster spart. Code
   aus `search.rs` entfernt, `docs/aspiration-windows.md` mit
   Smoke-Befund versehen, `tools/probe_aspiration.py` (umbenannt zu
   `probe_capture_ordering.py`) für künftige Versuche im Repo gelassen.
3. **MVV-Bonus / Centipawn-MVV — VERWORFEN am 16.05.2026.** Variante A
   (Centipawn-MVV mit LVA-Modifier) implementiert, Smoke + fastchess-
   SPRT 1000 Partien gegen pre-MVV-Baseline. A/B: +6.60 ± 16.76 Elo
   für MVV-CP, LOS 78 %, SPRT nicht entschieden (LLR −1.12 Richtung
   H0). Smoke zeigte +1 Quality-G (4 → 5) und +2 maxD, aber eine
   **Regression auf W5AboGf0** (Qxd6 → Qg4) — der ursprünglichen
   Cluster-1b-Anker-Stellung. Profil ähnlich Step 2 v2, aber bei
   Move-Ordering greift [[feedback-ab-vs-lichess-signal]] nicht
   (Spiegelstil ist hier ehrlich). Code aus `search.rs` entfernt
   (reproduzierbarer Revert), `docs/mvv-bonus.md` mit VERWORFEN-
   Status + Befund, Reproduktions-Binary `target/release/martuni-mvv-cp`
   und Match-Setup `matches/baseline_vs_mvv_cp/` aufgehoben.
4. **`connected_rooks_pair = 30` ausgerollt am 15.05.2026.** A/B-Match
   `matches/conn_rooks_150_vs_30` lief 15.05. 17:15–17:39, SPRT [0, 10]
   nach 240 Partien terminiert (H0 akzeptiert): **CR30 schlägt CR150
   um +150.65 Elo ±41.14**, LOS 100 % für CR30, Ptnml [45, 22, 43, 6, 4],
   PairsRatio 0.15, Total Time 00:23:54. Damit ist der Eval-Audit-
   Befund (92–99 % Bias-Treiber) live bestätigt — selfplay-Signal so
   deutlich, dass das übliche A/B-↔-Lichess-Caveat nicht greift.
   `eval.toml` im Repo-Root auf 30 gesetzt; kein Rebuild nötig
   (Laufzeit-Config). Lichess-Lookback am 16.05. Bei Plateau evtl.
   Folge-A/B 30 vs 0 oder 30 vs 60.
5. **NMP-Verfeinerungen** (adaptive R, Verification Search) — erst wenn
   die Endgame-Rate Anlass gibt; aktuell kein Druck.

## Offene Themen — Search

- **Futility / Reverse Futility Pruning** — Blattnähe-Pruning, wenn die
  statische Bewertung selbst mit großzügigem Margin Alpha nicht erreicht.
- **Lazy MovePicker** — inkrementelle Zuggenerierung (Hash → Captures →
  Killer → Quiet) statt vorab vollständig sortierten Vektor; spart
  Rechenzeit bei frühen Cutoffs.
- **LMR Variante B** — logarithmische Reduktionsformel mit Lookup-Table
  als A/B-Test gegen Variante A (siehe [lmr-plan.md](lmr-plan.md)).
- **LMR auch in PV-Knoten** — Stockfish-Stil mit konservativeren
  Reduktionswerten (siehe [lmr-plan.md](lmr-plan.md)).

## Offene Themen — Evaluation

- **Backward Pawns** — Strafe für Bauern ohne Nachbarbauern hinter sich,
  deren Vorrückfeld vom Gegner sicher kontrolliert wird.
- **Outposts (Springer)** — Bonus für Springer auf gedeckten Zentralfeldern,
  die durch gegnerische Bauern nicht mehr vertrieben werden können (siehe
  [see.md](see.md), Abschnitt „Offene Schritte").
- **Dynamischer Bishop-Pair-Bonus** — Bonus skaliert mit Brett-Offenheit
  (umgekehrt proportional zur Bauernanzahl). Aktuell statischer Fixwert
  `bishop_pair_each`.
- **Pawn-Endgame-Guard** — Opposition, Key Squares und Rook-Pawn-Edge als
  ergänzendes Wissen zum bereits vorhandenen `kpk_score` in
  [endgame.rs](../src/endgame.rs). Konzept (Variante B, additiver Eval-
  Term in `eval.rs`) liegt in
  [pawn-endgame-guard.md](pawn-endgame-guard.md) — 23.05.2026 ausgelöst
  durch den stickshark99-Deep-Dive (KNP-Endspiel-Drift).
- **Tapering für Passbauern und isolierte Bauern** — Passbauer-Bonus
  per Rang ist da (`pawn_passed_rank_bonuses`), expliziter MG/EG-Split
  fehlt; isolierte Bauern sind phasenflach mit −20 cp (siehe
  [eval-kalibrierung.md](eval-kalibrierung.md), Punkte 2 und 3).
- **Springer- vs. Läufer-Differenzierung** — N=B=300 cp ist nicht
  stellungsabhängig; Plan in
  [vorbereiteter_Prompt_dynamische_Figurenbewertung.md](vorbereiteter_Prompt_dynamische_Figurenbewertung.md).
- **Bishop-Trap-Detection** (siehe [see.md](see.md)).
- **Pawn-Shield bei nach vorne gegangenem König** — kleine Schwäche, in
  [eval-kalibrierung.md](eval-kalibrierung.md) notiert.

## Offene Themen — Performance / Code-Qualität

- **SEE inkrementell** — `all_attackers_to` cachen statt pro Schlag neu
  berechnen (siehe [see.md](see.md)).
- **`Score`-Struct für Tapered Eval** — `(i32, i32)`-Tupel ablösen;
  `Add`/`Sub`/`Mul`-Traits, finale Interpolation per `score.taper(phase)`.
- **Iterative Deepening auslagern** — eigener `SearchState`-Methode, damit
  `search()` schlanker wird.
- **Benannte Ordering-Konstanten** — magische Zahlen in
  `order_moves` durch benannte Konstanten oder ein Stage-Enum ersetzen.
- **`is_passed_simple` (search.rs) vs. `is_passed` (eval.rs)** sind nach
  dem 09.05-Bitmask-Refactor byte-identisch. Bei Gelegenheit
  konsolidieren (eine Instanz, gemeinsamer Aufrufer).

## Verlauf

*Chronologische Zusammenfassung der bereits umgesetzten Maßnahmen.
Details und Mess-Daten in den verlinkten Dokumenten.*

- **Dynamische Figurenbewertung Schritt 3 — Bishop-Pair-Tapering
  (16.05.2026) — DONE.** Statisches `2 * bishop_pair_each = 30 cp`
  ersetzt durch phasen-getaperten Term mit Offenheits-Skala in
  `evaluate_side` und `evaluate_side_breakdown` (`src/eval.rs`):
  `eg_value = bishop_pair_eg + (16 - total_pawn_count) * bp_open_scale`,
  dann `taper(bishop_pair_mg, eg_value, phase)`. Drei neue Felder in
  `EvalParams` (`bishop_pair_mg/eg`, `bp_open_scale`) mit Defaults
  30/30/0 — ohne TOML-Override identisch zum alten Verhalten. Wirksame
  Werte aus `eval.toml [material_dynamic]`: Variante A 30/50/2 (MG
  unverändert, EG +20, +2 cp pro fehlendem Bauer). Sanity-Check vor
  Match: MG/Phase 23 → 30 cp, EG/Phase 2 → 68 cp.
  A/B-Match `matches/baseline_vs_dynmat_step3/`: 1000 Partien
  5+0.05 UHO_Lichess_4852_v1, Hash 64 MB, SPRT [0, 10] nicht
  terminiert. **Ergebnis +6.95 Elo ±18.06 für Step3, LOS 77.5 %**,
  411W/158D/431L Baseline-Sicht, DrawRatio 44.8 %, PairsRatio 0.85,
  LLR −1.10. Trotz CI-deckt-Null ausgerollt analog Step 2 v2
  ([[feedback-ab-vs-lichess-signal]]): Selfplay-Spiegelstil ist für
  Eval-Material-Hebel nicht das definitive Signal, Lichess-Lookback
  entscheidet. Rollout 22:23 als Hot-Replace (kein laufendes Spiel),
  `target/release/martuni.backup-pre-step3-20260516` als Backup-Binary.
  Konzept: [dynmat-step3.md](dynmat-step3.md). `bishop_pair_each = 15`
  bleibt als toter Anker im Code (Konsistenz mit Step-1-Pattern, keine
  externe Referenz). Lookback offen.
- **Debug-Eval-Breakdown + `connected_rooks_pair`-Befund (15.05.2026) — DONE.**
  Neues UCI-Kommando `eval` druckt jede Eval-Komponente als
  `info string`-Zeile (`src/eval.rs` `evaluate_breakdown`,
  `print_eval_breakdown`; rein additiv, alle Tests grün). Damit
  Validierungs-Tool `tools/validate_connected_rooks.py` über
  mehrere Stellungen gefahren. Befund: `connected_rooks_pair = 150`
  erklärt **92–99 %** des Eval-Bias in beiden echten Cluster-1b-
  Stellungen — `W5AboGf0` post Qxd6 Nxd6: Bias +161 cp, davon
  +150 vom Term; `54iwUiMx` m25 vor Ne7: Bias −162 cp, davon −150
  vom Term. Nach Abzug Rest-Gap ±11–12 cp (Rauschen). Standard-
  Mittelspiel zeigt deutlich anderes Bild (Rest-Gap −564 cp) —
  separates Thema, nicht hier. Konsequenz: A/B-Match
  `connected_rooks_pair = 30` vs 150 gestartet. Logs:
  `validate_connected_rooks_2026-05-15.txt`,
  `eval_breakdown_w5abogf0_2026-05-15.txt`,
  `quiesce_trace_w5abogf0_2026-05-15.txt`.
- **Cluster-1-Stichprobe `missed_capture` (15.05.2026) — DONE.**
  9 Stellungen aus dem 15.05-Auswertungsfenster auf Engine-Verhalten
  abgeklopft (`tools/probe_missed_captures.py`, je 30 s / d ≤ 14):
  3/9 lösen sich ab d4–d10 (Such-/Zeitdruck-Problem, kein Eval-Bug),
  4/9 bleiben bis d10 falsch, 2/9 oszillieren. Sanity-Check via
  Stockfish d30 (`tools/verify_cluster1b_stockfish.py`) reduziert die
  4 echten Kandidaten auf **2 belastbare Bugs** — `m1oQlfmG` war
  Artefakt der d17-Momentaufnahme (Stockfish flippt bei d30 selbst auf
  `Be2`). Übrig: `54iwUiMx` m25 (`Nxd4`, SF-Score läuft −66→0 cp) und
  `W5AboGf0` m29 (`Qxd6`, −62→−32 cp). Deep-Dive an W5AboGf0
  (`tools/trace_w5abogf0.py`): Free-Search 120 s / 100 M Knoten
  erreicht d10 und bleibt bei Qg4 (−49 cp); Post-Move-Eval pro Kandidat
  einzeln zeigt Qxd6 = +47 cp vs Qg4 = −44 cp (Diff 91 cp). SEE
  rechnet Q-für-Q-Capture korrekt mit Netto 0. **Befund: Search-
  Tiefe-/Move-Ordering, kein statischer Eval-Bug.** Daraus die
  Priorisierung in "Nächste Schritte": Aspiration Windows hochgezogen.
  Logs: `probe_cluster1_2026-05-15.txt`,
  `verify_cluster1b_sf_2026-05-15.txt`,
  `trace_w5abogf0_2026-05-15.txt`.
- **Buchlücken-Patches stickshark99-Block (15.05.2026) — DONE.**
  Vier zusätzliche Einträge in `tools/build_book_patches.py` →
  `src/polyglot/martuni_patches.bin` (jetzt 6 Einträge): `7…c5` statt
  `Bxc3+` (J6OoNBPQ), `10…Nge5` statt `Nxf2` (B8ZDeiEi), `11…a5` statt
  `O-O-O` (PLLnwdrh), `12.Nb5` statt `Nc4` (FdSEbZbU). Anders als die
  bisherigen Patches einmalige FENs aus 16 stickshark99-Partien — Wert
  niedriger, aber für frühe Mittelspielzüge plausibel; Reviewhinweis
  im Patch-Kommentar (entfernen falls keine Recurrence).
- **Auswertung 15.05.2026 (144 Partien post Dynmat-Step2 v2) — DONE.**
  Sample 13.05. (post-Deploy) — 15.05. 14:40. Lichess Blitz 2027 → 2010
  (−17), Rapid 2075 → 2083 (+8); netto flach im Rauschen, kein Rollback-
  Trigger erfüllt. **Entscheidung: Step 2 v2 behalten.** Σ 1.69
  Blunder/Partie (243 total). `missed_mate` stabil 0.035/P,
  `allows_mate` 0.188/P ist Artefakt aus bereits verlorenen Stellungen
  (alle 27 Fälle `eval_before < −300cp`, 7 davon aus einem einzigen
  Endspiel-Shuffling-Spiel `pD9ZAV3G`). `hangs_bishop` 0.109 → 0.139/P
  leichter Drift (11 in offenem Spiel), im Auge behalten. Neue
  Auffälligkeit: `missed_capture` 0.181/P, 13 davon in offenem Spiel —
  zwei Cluster (Capture-vs-Capture-Ordering und Forcing-Sac-Pruning).
  Neuer Sparring-Hotspot `stickshark99` (16 Spiele, 2.25 B/P, davon 5
  Eröffnungspatzer); `sxphia` von 1.7 auf 1.42 B/P gefallen. Datei:
  `analyse-13.05.2026.json`. Backup-Binary
  `target/release/martuni.backup-step1-20260513` bleibt aufgehoben.
- **Auswertung 11.05.2026 (137 Partien post Dynmat-Step1) — DONE.**
  Sample 10.05 22:17 — 12.05 19:05, alle mit Dynmat-Step1 live. Mate-
  Metriken weiter gesunken: `missed_mate`/Partie 0.038 → **0.029**,
  `allows_mate` 0.109 → **0.080**. Roadmap-Vorhersage eingelöst:
  `hangs_bishop` 0.120 → **0.109** (−9 %), `trade_down` 0.082 → 0.088
  ≈ flat. Lichess Blitz 1969 → 2027 (+58), Rapid 2032 → 2075 (+43). Σ
  Blunder/Partie 1.23 → 1.98 (+61 %), aber Gegner-Mix verzerrt
  (HynobiusChess 19×, black_numba 15×, TiSchachBot 13× → Bot-Turnier mit
  wenigen Teilnehmern, höheres Rating zieht härtere Stellungen). `hangs_
  knight` 0.066 → 0.190 ist überwiegend MG-Taktik in bereits gewonnenen
  Stellungen, kein Trade-Calculus-Effekt — vor Step 2 keine Eval-
  Reaktion. Datei: `analyse_11.05.2026.txt`.
- **Buchlücken-Patch EpimetheusBot 11. Nbd4 (12.05.2026) — DONE.**
  Zweiter Eintrag in `tools/build_book_patches.py` /
  `src/polyglot/martuni_patches.bin` (Polyglot-Hash
  `0xecaa75f8ae670fcc`): in `r1bk1b1r/p3pppp/2p2n2/1N6/1nP5/5N2/PP3PPP/
  R1B1KB1R w KQ - 0 11` spielt Martuni jetzt **Nbd4** statt **Na3**. Das
  alte `Na3` lief 5× in 137 Partien (≈200 cp Verlust pro Partie) — neben
  dem sxphia-Bxc5-Eintrag der zweite EpimetheusBot-typische Wiederholer.
  Verifikation: `info string book hit / bestmove b5d4`.
- **Dynamische Figurenbewertung Schritt 1 (10.05.2026) — DONE.** MG/EG-
  Tapering nur für Springer und Läufer im Per-Figur-Materialscore
  (`piece_material(piece, p, phase)` in `eval.rs`). Statische `p.knight` /
  `p.bishop` bleiben Anker für `king_exposure_penalty` (NPM-Schwelle 1500cp)
  und `endgame::strong_material` — Eigene Vorgabe von Tobias, damit der A/B-
  Test sauber ist und nicht versehentlich indirekte Eval-Drift entsteht.
  Neue Sektion `[material_dynamic]` in `eval.toml` mit Werten
  knight_mg=310 / knight_eg=290 / bishop_mg=305 / bishop_eg=320 — Kaufman-
  konsistent: N–B-Differenz ≈ 0 cp im MG, ≈ −30 cp im EG. Defaults im Code
  sind 300/300/300/300, ohne Override neutral. Mess-Setup: fastchess
  (`~/tools/fastchess`) + UHO_Lichess_4852_v1.epd (`~/tools/openings`).
  A/B-Match 1000 Partien (5+0.05, UHO, fastchess SPRT [0,10]):
  Dynmat +11.47 Elo ± 17.30, LOS 90.35 %, 402W / 163D / 435L Baseline-
  Sicht. SPRT formal nicht entschieden (Effekt unter +10-Schwelle), aber
  positive Tendenz konsistent mit dem 200-Spiele-Vorlauf (+15.65 Elo) — als
  bestanden gewertet. Schritte 2 (Pawn-Adjustment) und 3 (dynamisches
  Bishop-Pair) zurückgestellt bis Lichess-Validierung. Match-Outputs
  unter `matches/baseline_vs_dynmat_step1*/` (gitignored).
- **Buchlücken-Patch (10.05.2026) — DONE.** `src/polyglot/martuni_patches.bin`
  als Polyglot-Datei mit Vorrang vor den externen Büchern. Erster Eintrag:
  `9...Bc7 statt 9...Bxc5` in der wiederkehrenden sxphia-Stellung
  (`r1bq1rk1/1p1n1ppp/p1pbpn2/2Pp4/3P2P1/2N1PN1P/PPQ2P2/R1B1KB1R b KQ - 0 9`),
  Martuni hatte dort viermal in Folge denselben hangs_bishop-Blunder
  gespielt (Verlust ~270 cp pro Partie). Generator: `tools/build_book_patches.py`
  (python-chess, gegengeprüft am Polyglot-Startpos-Hash). `.gitignore`-
  Ausnahme für die Patch-Bin, externe Bücher bleiben ungetrackt.
- **Hotpath-Cleanup (09.05.2026) — DONE.** PR `perf-hotpath-logic-cleanup`,
  ausgelöst durch Beobachtung in Shredder-GUI auf dem Windows-Host: Martuni
  blieb bei Tiefe 7, Komodo 12 schon bei 17. Codex-PR im Review identifiziert
  und gemerged. Wesentliche Änderungen:
  - **Redundante MoveGen** in `alpha_beta` und `quiescence` entfernt:
    `board.status()` baut intern selbst `MoveGen::new_legal()`, danach lief
    derselbe Generator nochmal fürs Ordering. Jetzt: einmalige
    Generation, Terminal-Erkennung anhand `legal_moves.len() == 0` (vor NMP
    eingehängt, damit Stalemate/Checkmate nicht durchs Null-Move-Pruning
    fallen).
  - **Repetition-/TT-Hash** von `polyglot_hash` (iteriert alle 64 Felder) auf
    `Board::get_hash()` umgestellt — die `chess`-Crate pflegt diesen
    Zobrist-Hash inkrementell in `make_move`. Polyglot-Hash bleibt nur
    fürs Eröffnungsbuch zuständig.
  - **`check_ext`** wird nicht mehr unkonditional berechnet (sondern nur,
    wenn das Kind tatsächlich im Schach steht).
  - **`is_passed`/`is_passed_simple`** und **`rooks_connected`** auf
    Bitmask-Operationen umgebaut, kein `Vec`/`Square`-Loop mehr.
  - **`endgame::signature`** mit Fast-Exit `pawn_total > 1 → None`.
  - **`target-cpu=native`** korrekt nach `.cargo/config.toml` als rustflag —
    in `[profile.release]` wurde der Schlüssel von Cargo ignoriert
    (Compiler-Warnung "unused manifest key").
  - Verifikation Mittelspiel-Stellung `r4rk1/1bqnppbp/p2p1np1/1pp5/3PP3/
    2NBBN1P/PPPQ1PP1/R4RK1 w` mit `go movetime 12000`: Master Tiefe 6 in
    8.7 s (Tiefe 7 nicht erreicht), neuer Build Tiefe 7 in 9.1 s. NPS
    ≈1.6 M → ≈2.8 M (+64 %). Scores und Best-Moves auf jeder Tiefe
    identisch — reine Geschwindigkeit, keine Logikänderung.
- **Auswertung 09.05.2026 (183 Partien) — DONE.** Tiefenrelevante Motive
  weiter rückläufig: `allows_mate`/Partie 0.126 (04.05) → **0.109** (-13 %);
  `missed_mate`/Partie 0.057 → **0.038** (-33 %, sehr nah am ursprünglichen
  Ziel 0.03). Lichess Blitz 1965 → 1969 (+4), Rapid 2016 → 2032 (+16) —
  unterhalb der LMR-Zielspanne +30 bis +60, aber das Mess-Intervall enthält
  zwei zwischenzeitliche Bug-Fixes (Repetition-Detection 02.05, TT-Cutoff
  07.05) plus den Compiler-Tweak vom 06.05; klare Attribution
  schwierig. Neue Hotspots im Mittelspiel: `unclassified` 91 (40 %),
  `positional_collapse` 27, `hangs_bishop` 22 — typischer Mittelspiel-
  Bodensatz, kein klares Strukturproblem. Datei: `analyse_09.05.2026.txt`.
- **TT-Cutoff bei Repetition-Vergiftung (07.05.2026) — DONE.** Reproduzierer
  PGQZhMjF (Wojtmic-Bot vs Martuni, 06.05.): die TT speichert nur den
  Stellungs-Schlüssel, keinen Repetition-Kontext. Wenn dieselbe Stellung im
  Spielverlauf bereits aufgetaucht war, kann ein gespeicherter Mate-/
  Cutoff-Score stale werden — der frühere Pfad führte zu echtem Mate, der
  jetzige läuft durch eine 3-fold-Wiederholung. Fix in
  `alpha_beta`: bei TT-Hit prüfen, ob `key` im Slice
  `state.history[..root_history_len]` (Spielhistorie) liegt; wenn ja, nur
  Move-Hint übernehmen, Score-Cutoff unterdrücken. `slice.contains` ist
  O(n) auf einem Slice typisch < 200 Einträge und wird nur betreten, wenn
  überhaupt ein Cutoff-Kandidat anliegt — kein Hot-Path-Treffer. Spiegelbild
  des Repetition-Bugs vom 02.05.: die Repetition-Logik ist in beide
  Richtungen heikel. Tests: `search::tests::poisoned_tt_does_not_select_
  repeated_move`, `tt_cutoff_suppressed_when_key_in_game_history`.

- **Auswertung 01.05.2026 (162 Partien) — DONE.** Die 28.04-Anpassung hat
  geliefert: Endgame-Blunder/Partie 0.60 → 0.358, exposed_king 0.14 → 0.086,
  positional_collapse 0.24 → 0.160 (alle deutlich besser). Einziger
  Negativtrend: `missed_mate`/Partie 0.075 → 0.105. Inspektion der 17 Fälle
  zeigt: nahezu alle sind Stellungen, in denen Martuni schon klar gewann
  (`martuni=+6cp .. +21cp`) und nur das schnellste Matt nicht fand —
  strukturelles Tiefen-Problem, kein Eval-Fehler. Lichess-Rating: Blitz
  1864 → 1921, Rapid 1928 → 1975 (3 Tage). Befund hat NMP-Implementierung
  ausgelöst.
- **Eingemauerter Turm (`rook_trapped_endgame_penalty`) — DONE.** Am 28.04.
  mit `tools/diagnose_rook_trapped.py` geprüft: Term feuert in nur 2 von 104
  Endgame-Blundern (1.9 %), in beiden Fällen sachlich korrekt. Nicht der
  Treiber der Endgame-Verschlechterung — bleibt bei `-10`, kein Anlass zum
  Justieren.
- **Null-Move Pruning + PVS — DONE 01.05.2026.** Plan stand in
  [null-move-pruning.md](null-move-pruning.md) (NMP-Konzept), PVS wurde
  gleich mitgeliefert, weil NMP ohne Nullfenster-Knoten in der Suche nie
  greift. Verifikation: Mittelspiel-Stellung −43 % Knoten auf gleicher
  Tiefe; `missed_mate`-Stellung aus dem Analyse-File (Martuni vs Bot5551,
  Zug 29) wird jetzt mit `mate 6` gelöst statt vorher unentdeckt zu
  bleiben. R = 2 konstant, Mindesttiefe 3, Zugzwang-Schutz via
  `has_non_pawn_material`.
- **Auswertung 04.05.2026 (175 Partien) — DONE.** NMP-Effekt bestätigt:
  `missed_mate`/Partie 0.105 → **0.057** (−46 %), Lichess Blitz 1921 → 1965
  (+44), Rapid 1975 → 2016 (+41). Endgame-Rate stabil (0.358 → 0.337),
  keine Zugzwang-Regression. Neuer Hotspot `allows_mate` 0.126/P. (22 Fälle),
  primär Tiefenproblem in bereits verlorenen Stellungen. Details:
  `project_auswertung_2026_05_04` Memory, Datei `analyse_04.05.2026.txt`.
- **LMR implementiert (04.05.2026).** Variante A nach Tobias-Spezifikation:
  Stufenformel (R=1 ab depth≥3 & Index≥3, R=2 ab depth≥6 & Index≥6),
  nur Non-PV-Knoten, ab dem 4. sortierten Zug. Ausgeschlossen von Reduktion:
  Captures (über `sm.see_val`), Promotionen, Schachgebote, Züge im Schach,
  Killer-Moves, alle Züge mit aktiver Extension. Re-Search-Kaskade:
  reduzierte Nullfenster-Suche → bei Fail-High volle Tiefe Nullfenster →
  bei `alpha < score < beta` PVS-Re-Search mit vollem Fenster.
  Verifikation: `missed_mate`-Stellung Tiefe 9/mate 6 in 2.7 s mit 6.8 M
  Knoten (vorher Tiefe 7/mate 6 in 5.7 s mit 17.4 M Knoten). Wichtig beim
  Implementieren war: `.max(1)` auf `scout_depth` darf nur greifen, wenn
  tatsächlich reduziert wird, sonst wird der natürliche Übergang
  `new_depth==0` → Quiescence aufgebläht und die Suche kollabiert.
  Konzeption: [lmr-plan.md](lmr-plan.md). History-Heuristic bewusst NICHT
  als zusätzliches LMR-Kriterium — wirkt nur über die Zugreihenfolge.
- **Repetition-Detection korrigiert (02.05.2026).** `state.history.contains`
  zählte vorher 1-fold in Spielhistorie als Remis und blockierte ruhige
  Best-Moves (Repro: vGwmaXUy, 19.Ng5?? statt 19.Qe4). Neuer Helfer
  `is_repetition_draw` trennt Spielhistorie und Suchpfad
  (`SearchState.root_history_len`); Unit-Tests in `search::tests`.
- **Analyse-Skript verbessern — DONE 02.05.2026.** Pauschales
  `--min-movetime 0.3` ersetzt durch per-Klasse-Schwellen über
  `est = base + 40·inc` (Lichess-Konvention): bullet 0s, blitz 1s, rapid 3s,
  classical 5s. Schwellen sind `int` (PGN-Clocks haben Sekunden-Auflösung),
  `--min-movetime` (float, Default 0.3) bleibt als Fallback für fehlende /
  unparsbare `[TimeControl]`-Tags. Skip-Meldung auf stderr zeigt jetzt Klasse
  + Schwelle. Doku: [blunder-analyse.md](blunder-analyse.md) (Wartungshinweis
  „Movetime-Filter pro Zeitkontrolle (2026-05-02)").
- **Mobility-Term — DONE.** Variante B (Safe Mobility) in
  `eval.rs::mobility_score` implementiert; getapert zwischen MG/EG mit
  Defaults `knight 3/3, bishop 3/4, rook 2/5, queen 1/2`. Eingeführt nach
  der Analyse vom 21.04.2026 als Antwort auf den Mittelspiel-Bodensatz aus
  `unclassified` und `positional_collapse` (siehe [see.md](see.md),
  Abschnitt „Regression-Analyse 2026-04-21").
- **Turm auf 7. Reihe — DONE.** `eval.rs::rook_seventh_rank_bonus`.
- **SEE + Bad-Capture-Pruning + Killer/History — DONE.** Lange Mess- und
  Korrekturhistorie in [see.md](see.md) (April-Iterationen 12. → 21.).
- **Endspielmodul Phasen A/B/C — DONE.** Mop-up (KRvK, KQvK, KRRvK, KQQvK),
  KPK mit Square-Rule, KBNK mit Bishop-Color-Mattecken; siehe
  [endgame.md](endgame.md) und [endgame.rs](../src/endgame.rs).
