# Code-Review Martuni - Programmfluss, Hot Path und schachliche Weiterentwicklung

Datum: 2026-06-16

## Umfang

Gelesen wurden `CLAUDE.md`, die zentralen Engine-Module unter `src/` sowie die vorhandene Roadmap und Fachdokumentation unter `docs/`. Verifiziert wurde mit:

```bash
cargo test
```

Ergebnis: 90 Tests bestanden, 0 fehlgeschlagen.

Die Review bewertet drei Ebenen:

1. Programmfluss und vermeidbare Verzögerungen im Suchpfad.
2. Code-Sauberkeit, Kopplung und Fehlerrisiken.
3. Schachfachliche Erweiterungen mit geschätztem Nutzen.

Die Nutzenschätzungen sind bewusst als Bandbreiten formuliert. Ohne A/B-Selfplay, feste Tiefenvergleiche und NPS-Benchmarks bleiben sie Annahmen.

## Kurzfazit

Martuni hat eine schlüssige Grundarchitektur: UCI, Positionstracking, Suche, Evaluation, Endspielwissen, Polyglot und Syzygy sind sauber getrennt. Besonders positiv sind die bereits umgesetzten Hot-Path-Korrekturen: Wurzel-`MoveGen` wird nur einmal erzeugt, SEE-Werte werden in der Suche wiederverwendet, TT-Mate-Scores sind ply-korrigiert, und die Repetition-Logik trennt Spielhistorie von Suchpfad.

Die größten verbleibenden Hebel liegen nicht in offensichtlichen Doppelaufrufen, sondern in der Form der Hot-Path-Objekte:

- `Vec`-Allokation und vollständiges Sortieren in jedem inneren Suchknoten.
- `Mutex`-Zugriff auf die Transposition Table bei jedem Probe/Store.
- mehrfach neu berechnete Eval-Kontexte pro Blatt.
- fehlende Aspiration Windows und noch ausbaufähige TT-Ersetzungsstrategie ohne Generation/Age oder Cluster-Slots.
- Quiescence lässt stille Checks außerhalb von Schachstellungen noch aus.

Schachlich ist die Engine für ihre Größe schon breit ausgestattet. Der größte fachliche Gewinn liegt wahrscheinlich in besserer Selektivität der Suche und in Endspiel-/Pawn-Knowledge für 6-14-Steine-Stellungen, also genau oberhalb der aktiven 3-4-5-Syzygy-Abdeckung.

## Umsetzungsstand 2026-06-16

Diese Review-Umsetzung wurde bewusst auf kleine bis mittlere, gut testbare Punkte begrenzt. Die Live-Binary unter `target/release/martuni` wurde dabei nicht überschrieben; der Kandidat wurde separat mit `CARGO_TARGET_DIR=/tmp/martuni-target-review` gebaut.

Umgesetzt:

1. Syzygy effektive Max-Kardinalität wird aus den vorhandenen `rtbw`-/`rtbz`-Dateinamen abgeleitet und mit `pyrrhic_rs::max_pieces()` gekappt.
2. Quiescence reduziert die Capture-Iteration über Zielmasken inklusive En-passant-Zielfeld und nimmt ruhige Queen-Promotions in die taktischen Züge auf.
3. Die Transposition Table ersetzt Einträge depth-preferred und bevorzugt Exact-Entries gegenüber Bounds gleicher Tiefe.
4. Polyglot-Book-Probing erzeugt die legalen Root-Züge einmal und verwendet diese Liste zur Legalitätsprüfung aller Buchkandidaten.
5. UCI `position` meldet Fehler beim FEN-Parsing oder beim Anwenden der Zugliste per `info string position error: ...` und fällt auf die vorherige vollständige Position zurück.

Verifikation:

- `cargo test` mit separatem Target-Dir: 94 Tests bestanden, 0 fehlgeschlagen.
- Release-Build nach `/tmp/martuni-target-review/release/martuni`.
- A/B-Run über 1000 Partien, `old` gegen `new`, `5+0.05`, `Hash=64`, UHO-Eröffnungen: `old` 376 Siege, 438 Niederlagen, 186 Remis, 469.0/1000 Punkte = 46.90 %. Das entspricht aus Sicht der neuen Engine etwa `+21.57 +/- 17.12 Elo`. SPRT wurde nicht formal accepted: `LLR -2.23` bei Grenzen `(-2.94, 2.94)`.

Weiter offen:

1. Staged MovePicker statt vollständigem `Vec`-Sort in jedem inneren Knoten.
2. `Mutex`-Zugriff auf die Transposition Table aus dem Hot Path entfernen oder durch eine passende spätere Parallel-Sucharchitektur ersetzen.
3. TT-Generation/Age und optionale Cluster-Slots ergänzen; umgesetzt ist bisher nur die depth-/exact-preferred Ersetzungslogik.
4. Aspiration Windows erneut separat und feature-gegatet testen. Ein früherer Versuch ist in `docs/aspiration-windows.md` als negativ dokumentiert, daher wurde dieser Punkt in dieser Umsetzung nicht erneut aktiviert.
5. `EvalContext` refactoren und zunächst bit-exakt gegen feste Stellungssuiten absichern.
6. Zeitmanagement mit Soft-/Hard-Deadline, PV-Stabilität, Score-Drops, Root-Move-Anzahl und weiteren UCI-Zeitparametern ausbauen.
7. Quiescence um begrenzte stille Checks erweitern, nur mit Delta-/SEE-Gates gegen Knotenexplosion.
8. Endspielwissen oberhalb der Syzygy-Grenze, Bauernstruktur, Threat Evaluation und King Safety fachlich vertiefen.
9. Mess-Infrastruktur um feste NPS-/Node-Suiten, TT-Statistiken, LMR-/NMP-Zähler und Syzygy-Probe-Statistiken erweitern.

## Befunde Nach Priorität

### P1 - Move Ordering allokiert und sortiert an jedem inneren Knoten

Fundstellen: `src/search.rs:633`, `src/search.rs:1288`

`alpha_beta` erzeugt nach TT/NMP einen `MoveGen`, `order_moves` sammelt alle Züge in einen `Vec<ScoredMove>` und sortiert den ganzen Vektor per `sort_by_key`. Das ist einfach und korrekt, aber im Suchbaum ein klassischer Hot-Path-Kostentreiber: Die Engine braucht in der Regel nur "nächsten besten Zug", nicht zwingend eine vollständig sortierte Liste aller Züge.

Empfehlung:

- Einen staged MovePicker einführen:
  - TT-Move zuerst.
  - gute Promotions und gute Captures nach SEE/MVV.
  - Killer.
  - ruhige Züge mit History.
  - schlechte Captures zuletzt.
- Optional zuerst ohne neue Abhängigkeit mit wiederverwendbarem `Vec` im `SearchState`, später ggf. `SmallVec`/Array-Buffer.

Geschätzter Gewinn:

- NPS/Knotenzeit: +5 bis +15 % möglich.
- Spielstärke: +5 bis +20 Elo, vor allem wenn durch frühere Cutoffs mehr Tiefe erreicht wird.
- Risiko: mittel. Move-Ordering darf keine Legalität ändern; Verifikation über identische Resultate bei abgeschaltetem neuen Picker oder feste Teststellungen ist nötig.

### P1 - Transposition Table wird pro Probe und Store über `Mutex` gelockt

Fundstellen: `src/uci.rs:18`, `src/search.rs:579`, `src/search.rs:1002`, `src/tt.rs:44`

Die TT liegt als `Arc<Mutex<TranspositionTable>>` im SearchState. Da aktuell nur ein Suchthread läuft, ist der Mutex im Hot Path überwiegend Synchronisations-Overhead, nicht echte Parallelitätsabsicherung. Pro innerem Knoten gibt es mindestens einen Probe-Versuch und später oft einen Store.

Empfehlung:

- Den TT-Zugriff aus dem Hot Path entkoppeln:
  - Variante A: TT während einer Suche exklusiv an den Suchthread binden und nur an UCI-Grenzen sperren.
  - Variante B: lock-freie/atomare TT-Slots für eine spätere Multi-Thread-Suche vorbereiten.
  - Variante C: mindestens `parking_lot::Mutex` messen, falls die Architektur zunächst bleiben soll.
- `stop`, `ponderhit` und Zeitsteuerung dürfen dabei unabhängig vom TT-Lock bleiben.

Geschätzter Gewinn:

- NPS/Knotenzeit: +2 bis +8 % bei Single-Thread-Suche.
- Spielstärke: +0 bis +10 Elo indirekt über Tiefe.
- Risiko: mittel bis hoch, weil UCI-Kommandos wie `ucinewgame`, `setoption Hash` und laufende Suche sauber koordiniert bleiben müssen.

### P1 - TT-Ersetzungsstrategie ist "replace always"

Fundstellen: `src/tt.rs:40`, `src/tt.rs:93`

`store` überschreibt den Slot immer. Dadurch kann ein tiefer, wertvoller Eintrag aus Iteration N durch einen flachen oder weniger hilfreichen Eintrag verdrängt werden. Das bremst spätere Iterationen: weniger TT-Cutoffs, schlechtere Hash-Moves, mehr Re-Searches.

Empfehlung:

- Depth-preferred Replacement einführen:
  - immer ersetzen bei leerem Slot oder anderem Key mit niedrigerer Tiefe.
  - Exact-Entries bevorzugen.
  - Generation/Age ergänzen, damit alte tiefe Einträge irgendwann weichen.
- Optional zwei Cluster-Slots pro Index statt einem Slot.

Geschätzter Gewinn:

- Knotenreduktion: +3 bis +15 % realistisch.
- Spielstärke: +5 bis +20 Elo möglich.
- Risiko: gering bis mittel. Falsche Replacement-Logik macht die Engine nicht inkorrekt, kann aber Move Ordering verschlechtern.

### P1 - Aspiration Windows fehlen noch im Iterative Deepening

Fundstelle: `src/search.rs:370`

Jede Iteration startet mit `-INF..INF`. Das ist robust, aber teurer als ein Fenster um den Score der vorherigen Iteration. Da Martuni bereits PVS, TT und stabile Iterationsergebnisse nutzt, ist das ein naheliegender nächster Suchhebel.

Empfehlung:

- Ab Tiefe 4 mit einem Fenster um `last_score` starten, z. B. +/- 25 cp.
- Bei Fail-Low/Fail-High exponentiell erweitern und erneut suchen.
- Mate-Scores und TB-Scores gesondert behandeln, damit kein künstlich enges Fenster entsteht.

Geschätzter Gewinn:

- Knotenreduktion: +5 bis +20 % in ruhigen Stellungen.
- Spielstärke: +5 bis +20 Elo möglich.
- Risiko: mittel. Falsch implementierte Re-Searches können Zeit verlieren oder instabile PVs erzeugen.

### P1 - Evaluation berechnet mehrere Stellungsfakten mehrfach

Fundstellen: `src/eval.rs:17`, `src/eval.rs:23`, `src/eval.rs:29`, `src/eval.rs:35`, `src/eval.rs:122`, `src/eval.rs:1300`, `src/eval.rs:1344`

`evaluate` ruft getrennt `pst_score`, `evaluate_side`, `mobility_score`, `king_activity_endgame`, `king_passed_pawn_synergy`, `pawn_endgame_guard`, `material_imbalance` und `material_deficit_damping` auf. Viele dieser Funktionen holen dieselben Bitboards, Materialzählungen, Bauernmengen und Passed-Pawn-Informationen erneut.

Das ist korrekt und modular, aber in Quiescence-Blättern teuer.

Empfehlung:

- Einen lokalen `EvalContext` pro `evaluate()` bauen:
  - Bitboards je Seite.
  - Piece counts und NPM.
  - Pawn masks, passed pawns, pawn attacks.
  - Phase.
  - optional Materialwerte je Seite.
- Danach alle Eval-Terme aus diesem Kontext speisen.
- Den Debug-Breakdown entweder denselben Kontext nutzen lassen oder bewusst separat halten.

Geschätzter Gewinn:

- Eval-Kosten: -10 bis -25 %.
- Gesamt-NPS: +2 bis +8 %, abhängig vom Anteil Quiescence/Leaf-Evals.
- Spielstärke: +0 bis +10 Elo indirekt über Tiefe.
- Risiko: mittel. Die Gefahr liegt in unbemerkten Bewertungsdrifts; daher zuerst bit-exakte Refactors mit festen Stellungssuiten.

### P1 - Quiescence filtert aus allen legalen Zügen nur Captures

Fundstellen: `src/search.rs:1025`, `src/search.rs:1033`

Im normalen Quiescence-Pfad wird `MoveGen::new_legal` erzeugt, danach werden nur Captures gesammelt und sortiert. Die `chess`-Crate bietet `set_iterator_mask`, das zwar die Legal-Move-Erzeugung nicht ersetzt, aber die Iteration über ruhige Zielquadrate reduzieren kann.

Wichtiger als die reine Geschwindigkeit: ruhige Damenumwandlungen werden aktuell nicht in der Quiescence betrachtet. Auch stille Checks fehlen, solange die Seite nicht bereits im Schach steht.

Empfehlung:

- Capture-Iteration über Zielmaske der gegnerischen Figuren reduzieren.
- En-passant-Zielfelder separat berücksichtigen.
- Nicht-schlagende Queen-Promotions in Quiescence aufnehmen.
- Optional stille Checks nur in den ersten 1-2 Q-Plies aufnehmen, mit engem Delta-/SEE-Gate.

Geschätzter Gewinn:

- NPS: +1 bis +4 % durch weniger Iteration/Filterung; bei echter Capture-Generierung mehr, falls später eigener Generator entsteht.
- Spielstärke: +2 bis +10 Elo durch ruhige Promotions; +5 bis +20 Elo durch vorsichtiges Check-QSearch, aber mit Explosionsrisiko.
- Risiko: mittel. Check-QSearch kann die Knoten stark aufblasen; Promotions sind risikoarm.

### P2 - Syzygy probt potentiell 6-7-Steiner, obwohl nur 3-4-5 geladen sind

Fundstellen: `src/syzygy.rs:137`, `src/syzygy.rs:237`, `docs/roadmap.md`

Die Roadmap notiert bereits, dass `pyrrhic_rs::max_pieces()` 7 melden kann, obwohl nur 3-4-5-Dateien vorhanden sind. `probeable` nutzt diesen Wert als Gate. Dann laufen in 6-7-Steine-Stellungen unnötige Probe-Versuche, die anschließend fehlschlagen.

Empfehlung:

- Echte maximale geladene Kardinalität aus vorhandenen Dateinamen ableiten.
- `max_pieces = min(pyrrhic_max, detected_max)` setzen.
- Optional als UCI-Info ausgeben: gemeldet vs. effektiv.

Geschätzter Gewinn:

- Gesamt: vermutlich <1 % NPS und kaum Elo.
- In 6-7-Steine-Endspielen ohne passende Tabellen: lokal spürbar, geschätzt +5 bis +20 % Endgame-NPS.
- Risiko: gering.

### P2 - Zeitmanagement ist robust, aber sehr einfach

Fundstellen: `src/search.rs:245`, `src/search.rs:1560`, `src/uci.rs:246`

`calculate_think_time` nutzt grob 1/30 der Restzeit plus 80 % Inkrement. Das ist ein guter Start, aber es berücksichtigt keine PV-Stabilität, keine Score-Sprünge, keine Anzahl legaler Root-Moves, keine `movestogo`-Angabe und keine getrennte Soft-/Hard-Deadline.

Empfehlung:

- Soft Deadline: normale Iteration abbrechen, wenn Budget erreicht.
- Hard Deadline: Sicherheitslimit, damit auch bei Instabilität nicht überzogen wird.
- Mehr Zeit geben bei:
  - Root-Bestmove-Wechsel in der letzten Iteration.
  - großem Score-Drop.
  - vielen legalen Root-Moves.
  - kritischen Endspielen ohne Syzygy.
- UCI-Parameter `movestogo`, `nodes`, `infinite` zumindest sauber parsen.

Geschätzter Gewinn:

- Spielstärke: +10 bis +30 Elo möglich, besonders Blitz/Rapid.
- NPS: kein direkter Gewinn, aber bessere Zeiteinteilung.
- Risiko: mittel. Zeitmanagement-Fehler verlieren Partien sofort; erst mit UCI-Smoketests und Lichess-nahen Replays testen.

### P2 - Polyglot-Book legalisiert jeden Kandidaten separat

Fundstellen: `src/polyglot/book.rs:106`, `src/polyglot/book.rs:113`, `src/polyglot/book.rs:144`, `src/polyglot/book.rs:179`

Für jeden Book-Eintrag wird `decode_move` aufgerufen; darin wird zur Legalitätsprüfung ein neuer `MoveGen::new_legal` iteriert. Das passiert nur an der Wurzel und nur bei Buchtreffern, also nicht im Such-Hot-Path. Trotzdem ist es eine unnötige Wiederholung.

Empfehlung:

- Einmal `Vec<ChessMove>` legaler Root-Züge in `BookSet::probe` erzeugen.
- Decodierte Book-Züge gegen diese Liste prüfen.

Geschätzter Gewinn:

- NPS/Elo: praktisch 0.
- Latenz bei Buchzug: minimal besser.
- Risiko: gering.

### P2 - UCI `position` verschluckt Fehler beim Anwenden der Zugliste

Fundstellen: `src/uci.rs:210`, `src/position.rs:54`

`handle_position` ignoriert das Ergebnis von `position.apply_moves`. Bei korrekten GUIs ist das harmlos. Bei defekten oder manuell eingegebenen Kommandos kann die Engine still auf einer partiell gesetzten Position weitersuchen.

Empfehlung:

- Fehler als `info string position error: ...` ausgeben.
- Bei Fehler auf vorherige vollständige Position zurückfallen oder das Kommando komplett verwerfen.

Geschätzter Gewinn:

- Elo: 0 im Normalbetrieb.
- Betriebsrobustheit: hoch bei Debugging/Tools.
- Risiko: gering.

## Gute Entscheidungen, Die Beibehalten Werden Sollten

- `src/search.rs:287`: Wurzelzüge werden nur einmal erzeugt und wiederverwendet.
- `src/search.rs:579`: TT-Cutoff wird bei Repetition-Kontext bewusst unterdrückt, aber der Hash-Move bleibt als Ordering-Hinweis erhalten.
- `src/search.rs:646`: Static Eval für Null Move wird erst nach TT/Terminal-Gates berechnet.
- `src/search.rs:1288`: SEE wird im Move Ordering berechnet und danach für Extensions/Pruning wiederverwendet.
- `src/search.rs:35` und `src/search.rs:49`: Mate-Scores werden korrekt für TT-Speicherung normalisiert.
- `src/syzygy.rs:278`: Syzygy-Dateien werden vor mmap auf Magic Bytes geprüft; das verhindert harte SIGBUS-Crashes durch kaputte Tabellen.
- `src/eval.rs:1519`: Der Eval-Breakdown ist ein gutes Diagnosewerkzeug und sollte bei größeren Eval-Refactors als Konsistenztest erhalten bleiben.

## Schachfachliche Erweiterungen

### 1. Staged MovePicker plus erweiterte History-Heuristiken

Aktueller Stand: TT-Move, Captures/SEE, Killer und History sind vorhanden.

Erweiterung:

- Countermove-Heuristic.
- Continuation History.
- Capture History statt nur SEE/MVV.
- Root-Move-Reordering nach vorheriger Iteration.

Erwarteter Gewinn:

- Spielstärke: +10 bis +30 Elo.
- Knotenreduktion: +5 bis +20 %.
- Risiko: mittel. Move Ordering ist stark stellungsabhängig und muss über Selfplay validiert werden.

### 2. Aspiration Windows als nächster Suchstandard

Aktueller Stand: Volles Fenster pro Iteration.

Erweiterung:

- Fenster um `last_score`.
- Fail-soft Re-Search mit wachsendem Fenster.
- Sonderfälle für Mate/TB.

Erwarteter Gewinn:

- Spielstärke: +5 bis +20 Elo.
- Knotenreduktion: +5 bis +20 %.
- Risiko: mittel.

### 3. Futility Pruning und Reverse Futility Pruning nahe den Blättern

Aktueller Stand: SEE-Pruning nahe den Blättern, NMP, LMR.

Erweiterung:

- Futility Pruning bei `depth <= 2` für ruhige Non-PV-Züge, wenn `static_eval + margin <= alpha`.
- Reverse Futility bei klarer Überlegenheit.
- Margins abhängig von Depth und Phase.

Erwarteter Gewinn:

- Knotenreduktion: +5 bis +25 %.
- Spielstärke: +0 bis +20 Elo; stark abhängig von Tuning.
- Risiko: mittel bis hoch, weil taktische Ressourcen wegfallen können.

### 4. Quiescence um Promotions und begrenzte Checks erweitern

Aktueller Stand: Captures, bei Schach alle legalen Züge.

Erweiterung:

- Ruhige Queen-Promotions immer prüfen.
- Checking Moves nur bei kleinem Q-Ply und mit Delta-Gates.
- Optional "recapture extension" oder recapture-priorisierte QSearch.

Erwarteter Gewinn:

- Spielstärke: +5 bis +20 Elo.
- Taktische Stabilität: hoch in Promotions-, Matt- und King-Safety-Stellungen.
- Risiko: mittel wegen möglicher QSearch-Explosion.

### 5. Endspielwissen oberhalb der Syzygy-Grenze

Aktueller Stand: Syzygy 3-4-5, handcodierte Mop-up/KPK/KBNK-Heuristiken und Pawn-Endgame-Guard.

Erweiterung:

- Effektive Syzygy-Grenze korrekt auf vorhandene Dateien kappen.
- 6-14-Steine-Endspiel-Eval stärken:
  - outside passer.
  - protected passer.
  - connected passers.
  - blockierte Passer.
  - König im Quadrat für mehrere Bauern.
  - falscher Läufer bei Randbauer.
  - Turmaktivität im Turmendspiel: seitliche Checks, König abschneiden, passiver Turm.

Erwarteter Gewinn:

- Spielstärke: +10 bis +35 Elo, besonders Rapid und technische Endspiele.
- Risiko: mittel. Viele Endspielregeln sind nicht linear additiv; falsches Tuning kann Umwege erzwingen.

### 6. Bauernstruktur ausbauen

Aktueller Stand: isolierte Bauern, Datei-Boni, Phalanx, Freibauer-Rangboni.

Erweiterung:

- Doppelbauern.
- rückständige Bauern.
- Pawn Islands.
- Hebel/Breaks gegen gegnerische Ketten.
- Kandidaten-Freibauern.
- Bauernmehrheit am Flügel.

Erwarteter Gewinn:

- Spielstärke: +5 bis +25 Elo.
- Besonders relevant gegen positionell stärkere Bots, bei denen reine Taktik nicht reicht.
- Risiko: mittel. Bauernboni sollten phase-getapert und mit Mobility/King-Safety abgestimmt werden.

### 7. Threat Evaluation und lose Figuren

Aktueller Stand: SEE wird primär für Captures in Suche/Ordering genutzt; statische Threats sind nur indirekt abgebildet.

Erweiterung:

- undefended pieces / loose pieces.
- angegriffene höherwertige Figur.
- taktische Drohungen gegen König und Dame.
- pinned pieces und overloaded defenders.
- safe checks als Eval- oder Search-Hinweis.

Erwarteter Gewinn:

- Spielstärke: +10 bis +40 Elo möglich.
- Risiko: hoch, wenn Threats doppelt mit Search-Ergebnissen zählen oder spekulative Opfer überbewerten.

### 8. King Safety fachlich vertiefen

Aktueller Stand: 3x3-Zone, Angreifergewichte, Safety Table, Shield, Exposure.

Erweiterung:

- Pawn storms gegen rochierten König.
- offene Linien zum König.
- fehlende Fluchtfelder.
- Angreifer in Nähe plus Verteidiger in Nähe, nicht nur Angreiferzahl.
- Pins auf Königslinie.
- phase- und materialabhängige Skalierung der Safety Table.

Erwarteter Gewinn:

- Spielstärke: +10 bis +30 Elo.
- Risiko: mittel bis hoch. King Safety ist ein häufiger Grund für Opfer-Fehleinschätzungen.

### 9. Automatisierte Mess-Infrastruktur ausbauen

Aktueller Stand: viele Unit-Tests, Analyse-Skripte und dokumentierte A/B-Läufe.

Erweiterung:

- feste NPS-Benchmark-Suite mit:
  - Startpos.
  - taktische Mittelspiele.
  - ruhige positionelle Stellungen.
  - 6-8-Steine-Endspiele ohne Syzygy-Treffer.
  - Tablebase-Stellungen.
- Pro Lauf erfassen:
  - nodes, qnodes, nps.
  - TT hit/cutoff rate.
  - beta cutoffs nach Move-Index.
  - LMR re-search count.
  - NMP cutoffs.
  - Syzygy probe attempts vs hits.
- Für jede Performance-Änderung erst feste Tiefe, dann Selfplay.

Erwarteter Gewinn:

- Elo: indirekt, aber hoch. Es verhindert Fehloptimierungen.
- Entwicklungsnutzen: sehr hoch.
- Risiko: gering.

## Empfohlene Reihenfolge

1. Syzygy effektive Max-Kardinalität kappen. Kleine Änderung, klarer Nutzen in Endspielen, geringes Risiko.
2. Quiescence um ruhige Queen-Promotions erweitern. Kleine fachliche Lücke, geringe Knotenmehrkosten.
3. TT-Replacement depth-preferred machen. Hoher Nutzen, begrenzter Scope.
4. Aspiration Windows einführen. Starker Suchhebel, braucht gute Tests.
5. MovePicker statt Full-Sort/Vec pro Knoten. Größter struktureller Hot-Path-Umbau.
6. EvalContext refactoren. Erst bit-exakt, dann neue Eval-Terme.
7. Zeitmanagement verbessern, sobald die Suche stabiler messbar ist.

## Konkrete Mess-Gates

Für jede Such-/Performance-Änderung:

- `cargo test`
- feste Tiefe auf 12-20 repräsentativen FENs:
  - bestmove gleich oder plausibel besser.
  - node count und nps dokumentieren.
  - keine unerklärten Mate-/TB-Differenzen.
- 300-Partien Smoke-Selfplay.
- erst bei neutral/positiv auf 1000+ Partien erweitern.

Für Eval-Änderungen:

- `eval`-Breakdown auf bekannten Problemstellungen speichern.
- Score-Deltas gegen alte Version tabellieren.
- Keine gleichzeitigen Änderungen an mehreren Eval-Konzepten ohne Feature-Toggle.

## Schlussbewertung

Der Code ist nicht chaotisch langsam; er ist an vielen Stellen bereits bewusst optimiert. Die nächsten Gewinne kommen daher weniger aus dem Entfernen einzelner doppelter Funktionsaufrufe, sondern aus engine-typischen Datenstrukturen: MovePicker, TT-Policy, EvalContext und präziserem Zeitmanagement.

Schachlich sollte Martuni als nächstes die Suche effizienter machen, bevor sehr viele neue Eval-Terme hinzukommen. Mehr Suchtiefe durch bessere Selektivität wirkt breit. Danach sind Endspielwissen oberhalb der Syzygy-Grenze, Bauernstruktur und Threat Evaluation die wahrscheinlich stärksten fachlichen Hebel.
