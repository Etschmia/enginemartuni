# Martuni — Roadmap

Zentrale Übersicht der nächsten Schritte und der bisherigen Maßnahmen.
Detail-Begründungen, Mess-Verläufe und Konzepte stehen in den verlinkten
Einzeldokumenten:

- Search: [lmr-plan.md](lmr-plan.md), [null-move-pruning.md](null-move-pruning.md), [see.md](see.md)
- Evaluation: [eval-kalibrierung.md](eval-kalibrierung.md), [endgame.md](endgame.md), [vorbereiteter_Prompt_dynamische_Figurenbewertung.md](vorbereiteter_Prompt_dynamische_Figurenbewertung.md)
- Tooling: [blunder-analyse.md](blunder-analyse.md)

## Aktueller Status

LMR umgesetzt am 04.05.2026 und lokal verifiziert (`lmr-plan.md`, Stufe 1).
Wartet auf Lichess-Auswertung nach ≥100 Partien (Stufe 3).

## Nächste Schritte

1. **Auswertung LMR (≥100 Partien)** — Ziel-KPIs:
   - `allows_mate`/Partie 0.126 → ~0.07
   - `missed_mate`/Partie weiter Richtung 0.03
   - Rating +30–60 Elo
   - Sekundär: keine Regressionen bei `missed_capture`, `exposed_king`,
     `positional_collapse`.
2. Bei positiver Auswertung: **dynamische Figurenbewertung** in 3 Phasen —
   siehe [vorbereiteter_Prompt_dynamische_Figurenbewertung.md](vorbereiteter_Prompt_dynamische_Figurenbewertung.md).
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

## Verlauf

*Chronologische Zusammenfassung der bereits umgesetzten Maßnahmen.
Details und Mess-Daten in den verlinkten Dokumenten.*

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
