# Martuni — Roadmap

Zentrale Übersicht der nächsten Schritte und der bisherigen Maßnahmen.
Detail-Begründungen, Mess-Verläufe und Konzepte stehen in den verlinkten
Einzeldokumenten:

- Search: [lmr-plan.md](lmr-plan.md), [null-move-pruning.md](null-move-pruning.md), [see.md](see.md)
- Evaluation: [eval-kalibrierung.md](eval-kalibrierung.md), [endgame.md](endgame.md), [vorbereiteter_Prompt_dynamische_Figurenbewertung.md](vorbereiteter_Prompt_dynamische_Figurenbewertung.md)
- Tooling: [blunder-analyse.md](blunder-analyse.md)

## Aktueller Status

**Dynmat-Step2 (v2, Scales 2/2) am 13.05.2026 nach Lichess ausgerollt.**
A/B vs Step1 (1000 Partien, 5+0.05, UHO): Step2v2 nominell **+5.56 Elo
±18.89** (LOS ~72 %, CI deckt Null), v1-Variante (Scales 3/4) zuvor
+4.52 Elo bei ähnlich breitem CI. Beide A/B-Läufe lieferten kein
signifikantes Signal, **bewusste Entscheidung trotzdem auszurollen**:

- Step 1 zeigte im SPRT denselben Muster (+11.47 Elo, LOS 90 %, SPRT
  formal nicht bestanden), brachte aber auf Lichess klar messbare
  Verbesserung (Blitz +77, Rapid +20 nach 137 Partien).
- fastchess-Selfplay testet nur den Spiegel-Stil. Lichess deckt ein
  breiteres Stilspektrum (Maia, Greedy-Bots, positionelle Bots) ab —
  empirisch der bessere Indikator für Pawn-/Material-Hebel bei dieser
  Engine-Familie.
- Risiko ist klein und reversibel (Binary tauschen + Service-Restart).

Dynmat-Step1 hatte zuvor seinen Lichess-Trend bestätigt (Auswertung
11.05., 137 Partien): `missed_mate` 0.038 → **0.029**, `allows_mate`
0.109 → **0.080**, `hangs_bishop` 0.120 → **0.109**. Lichess-Rating
12.05. 19:00: Blitz 2027 (+77 seit 10.05), Schnellschach 2075 (+20).
Zweiter Buchlücken-Patch (11. Nbd4 vs EpimetheusBot) am 12.05. ergänzt.

## Nächste Schritte

1. **Lichess-Validierung Dynmat-Step2 v2** — ≥100–150 Partien post-Deploy
   beobachten (analog Step1-Validierung 11.05.). Rollback-Trigger:
   Blitz oder Rapid fällt >30 Punkte unter 2027/2075 und stabilisiert
   sich, ODER analyse_cron-Hotspots verschlechtern sich klar
   (`hangs_bishop`, `missed_mate`, `allows_mate`). Während des
   Lookbacks `eval.toml` nicht weiter anfassen.
2. **Dynamische Figurenbewertung Schritt 3** — dynamisches Bishop-Pair
   (Phase + Brett-Offenheit). Erst nach Step-2-Lichess-Validierung.
3. **NMP-Verfeinerungen** (adaptive R, Verification Search) — erst wenn
   die Endgame-Rate Anlass gibt; aktuell kein Druck.

## Offene Themen — Search

- **Aspiration Windows** — engeres Startfenster pro ID-Tiefe; spart
  Knoten bei stabiler Bewertung über aufeinanderfolgende Tiefen.
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
- **Pawn-Endgame-Guard** — Opposition in K+P-vs-K plus Square-of-the-Pawn,
  als ergänzendes Wissen zum bereits vorhandenen `kpk_score` in
  [endgame.rs](../src/endgame.rs).
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
