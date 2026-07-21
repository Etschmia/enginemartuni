# Martuni — Ideen-Umsetzungsanalyse (Stand 18.06.2026)

**Ziel:** Prüfung, ob alle in Roadmap, Code-Review (2026-06-16), weiteren docs/ und impliziten Ideen (CLAUDE.md, Notizen, git-Historie) genannten Konzepte bereits umgesetzt sind.  
**Quellen:** `docs/roadmap.md`, `docs/code-review-2026-06-16.md`, `docs/aspiration-windows.md`, `docs/lmr-plan.md`, `docs/null-move-pruning.md`, `docs/see.md`, `docs/mvv-bonus.md`, `docs/endgame.md`, `docs/syzygy-rust-options.md`, `CLAUDE.md`, git-Log (bis 970d3bb), src/-Module.  
**Methode:** Vergleich "Offen"-Listen vs. aktueller Code (grep + cargo test + git show), historische "DONE"-Einträge in Roadmap. Keine Code-Änderungen.

## Gesamtfazit
**Die meisten Ideen aus 2026-04 bis 2026-05 sind umgesetzt** (NMP + PVS, LMR-Variante A, SEE/MVV, King-Safety, Pawn-Shield, Tapered PST, Polyglot, Repetition-Fixes, TT ply-adjust, Syzygy Phasen ①–④ + Guard, Hotpath-Cleanup Bundle 1, Code-Review-Fixes vom 16.06.).

**Verbleibende offene Ideen** stammen primär aus dem heutigen Code-Review (Post-Commit-Status) und ergänzenden Roadmap-Punkten. Keine großen "vergessenen" Konzepte aus älteren docs/ (z. B. dyn. Figurenbewertung, MVV-Bonus bei SEE=0) sind noch unerledigt – diese wurden entweder umgesetzt oder bewusst zurückgestellt (Aspiration).

**Git-Historie** bestätigt: Letzter Commit (`970d3bb`) hat exakt die 5 Review-Umsetzungen (Syzygy max_pieces, Quiescence Capture-Mask + ruhige Promotions, TT depth-preferred + Exact-Prio, Polyglot Legal-Check einmalig, UCI position error) integriert. Keine uncommitted offene Logik.

## Offene Ideen — nach erwartetem Gewinn (Elo / NPS / Robustheit) sortiert

Priorisierung basiert auf Review-Schätzungen, Hot-Path-Auswirkung, typischen Schachengine-Hebeln und Risiko (bit-exakt vs. Verhaltensänderung). Gewinn-Schätzungen sind Bandbreiten ohne aktuelle A/B-Daten.

### 1. Sehr hoher Gewinn (primäre Hebel, +10–25 Elo + NPS)
- **Staged MovePicker statt vollst. Vec<ScoredMove> + sort_by_key pro innerem Knoten**  
  Fundstelle: `src/search.rs:633/1288` (order_moves).  
  Status: ✅ **UMGESETZT (19.06.2026, Branch `dev/engine-arbeit`)** — lazy `MovePicker` mit 8 Stufen ersetzt `order_moves`. Bit-exakt verifiziert (Node-Counts über 8 diverse Stellungen × alle Tiefen identisch zur Baseline, 94/94 Tests grün); gemessen **+8–12 % NPS** (best-of-6, identische Knoten). Rollout (Merge → master, Live-Build, Service-Neustart) erst nach dem laufenden 12h-Bullet-Turnier. Details: `docs/roadmap.md` (19.06.).  
  Erwarteter Gewinn: +5–15 % NPS (weniger Allokation/Sort), +5–20 Elo (frühere Cutoffs → mehr Tiefe). → NPS-Schätzung bestätigt (+8–12 %).  
  Risiko: Mittel (Legalität muss identisch bleiben; Verifikation über feste Teststellungen + bit-exakte Vergleiche). → eingelöst: Node-Count-Gleichheit beweist identische Legalität/Reihenfolge.  
  Priorität: P1 (Code-Review).

- **Zeitmanagement: Soft-/Hard-Deadline, PV-Stabilität, Score-Drops, Root-Move-Anzahl, UCI-Parameter**  
  Status: Basis-UCI-Zeit (movetime, wtime etc.) vorhanden; erweiterte Logik fehlt.  
  Erwarteter Gewinn: Hoher Elo-Gewinn in praktischen Partien (weniger Zeitnot-Fehler, bessere Zeitnutzung bei instabilen PVs). +5–15 Elo realistisch.  
  Risiko: Niedrig (kann feature-gated + mit Zeit-Suites getestet werden).  
  Priorität: Hoch (direkter Spielstärke-Hebel).

- **Endspielwissen oberhalb Syzygy (6–14 Steine): Bauernstruktur, Threat Evaluation, vertieftes King Safety, Pawn-Endgame-Guard**  
  Status: Syzygy 3-4-5 aktiv + alte Endgame-Heuristiken (endgame.rs, pawn-endgame-guard.md); 6–14-Steine-Bereich noch auf generische Eval angewiesen.  
  Erwarteter Gewinn: Deutliche Reduktion von Endgame-Blunders (Review: Gros der Blunder in 8–14 Steinen). +8–20 Elo + bessere Konversion.  
  Risiko: Mittel (Eval-Änderungen brauchen Kalibrierung).  
  Priorität: Hoch (Roadmap + Review Prio 8).

### 2. Hoher Gewinn (Mittel-Hot-Path + Korrektheit, +5–15 Elo / Overhead-Reduktion)
- **Mutex-Zugriff auf TT aus Hot-Path entfernen (oder exklusiv an Thread binden)**  
  Fundstelle: `src/uci.rs:18`, `search.rs:579/1002`, `tt.rs:44` (Arc<Mutex<...>>).  
  Status: Noch aktiv (ein Thread → reiner Overhead).  
  Erwarteter Gewinn: 2–8 % NPS (weniger Lock-Contention), indirekt + Elo durch mehr Knoten.  
  Risiko: Niedrig (aktuell Single-Thread).  
  Priorität: P1 (Review).

- **TT-Generation/Age + optionale Cluster-Slots (bessere Ersetzungsstrategie)**  
  Status: Nur depth-preferred + Exact > Bounds umgesetzt (heutiger Commit). Generation/Age fehlt.  
  Erwarteter Gewinn: Höhere TT-Hitrate, weniger veraltete Einträge → stabilere Cutoffs (+3–10 Elo).  
  Risiko: Niedrig–mittel.  
  Priorität: Mittel-hoch.

- **Quiescence: begrenzte stille Checks (mit Delta-/SEE-Gates)**  
  Status: Quiescence verbessert (heutiger Commit: Capture-Mask + ruhige Q-Promotions), aber stille Checks noch ausgeschlossen.  
  Erwarteter Gewinn: Bessere Taktik-Erkennung in ruhigen Stellungen (+3–8 Elo).  
  Risiko: Mittel (Explosionsgefahr ohne Gates).  
  Priorität: Review-Punkt 7.

### 3. Mittlerer Gewinn (Maintainability / Messbarkeit / zukünftige Hebel)
- **EvalContext refactoren + bit-exakte Absicherung gegen Stellungssuiten**  
  Status: Mehrfach neu berechnete Kontexte pro Blatt (Review-Befund).  
  Erwarteter Gewinn: Weniger Redundanz, bessere Testbarkeit, Vorbereitung auf dynamische Eval. +2–5 Elo indirekt.  
  Risiko: Niedrig (bit-exakt zuerst).  
  Priorität: Mittel.

- **Mess-Infrastruktur: feste NPS-/Node-Suiten, TT-Statistiken, LMR/NMP/Syzygy-Probe-Zähler**  
  Status: Fehlt (Review-Punkt 9). Aktuell nur ad-hoc analyse-*.json.  
  Erwarteter Gewinn: Ermöglicht präzise A/B-Tests und Tuning (langfristig +5–10 Elo durch bessere Daten). Kein direkter Spielstärke-Gewinn.  
  Risiko: Sehr niedrig.  
  Priorität: Niedrig-mittel (Enabler).

### 4. Niedriger / zurückgestellter Gewinn (früher negativ oder niedrige Priorität)
- **Aspiration Windows (erneut testen, feature-gated)**  
  Status: Früherer Versuch in `docs/aspiration-windows.md` als negativ dokumentiert (Re-Searches bei typischen ±90 cp Score-Sprüngen, Key-Stellung W5AboGf0 kaputt). Code entfernt.  
  Erwarteter Gewinn: Bei aktueller Eval-Charakteristik gering bis negativ. Nach Eval-Stabilisierung evtl. +δ=100 cp Variante prüfen.  
  Risiko: Mittel-hoch (kann aktuelle Stabilität zerstören).  
  Priorität: Niedrig (erst nach anderen Eval-Verbesserungen).

- **En-passant-Behandlung in Syzygy-Probe (v1-Skip)**  
  Status: Roadmap "Offen" (15.06.). Aktuell korrektes Auslassen.  
  Erwarteter Gewinn: Sehr gering (harmlos, selten relevant).  
  Risiko: Niedrig.  
  Priorität: Optional.

- **Lichess-Lookback / max_pieces-Kappung (letztere im heutigen Commit umgesetzt)**  
  Status: max_pieces abgeleitet → umgesetzt. Lookback = Monitoring-Aufgabe (kein Code).  
  Erwarteter Gewinn: Monitoring (kein direkter Gewinn).

## Zusammenfassung der Umsetzungsquote
- **Umgesetzt (historisch + 16.06.):** ~85–90 % der dokumentierten Ideen (Suche, Eval, Syzygy, Hotpath, TT-Basics, Polyglot, UCI-Verbesserungen).
- **Noch offen:** 9 Punkte aus Code-Review + 2–3 kleine Roadmap-Nebenpunkte.
- **Keine "vergessenen" Ideen** aus älteren Konzept-Docs (z. B. KonzeptDynamischeFigurenbewertung, dynmat-step3) – diese sind entweder in eval.rs/pst.rs integriert oder bewusst nicht priorisiert.

## Empfehlung für nächste Schritte (rein analytisch)
1. **P1 MovePicker + Zeitmanagement** zuerst (höchster ROI).
2. TT-Optimierungen parallel (niedriges Risiko).
3. Endspiel- und Quiescence-Erweiterungen nach Mess-Infrastruktur (bessere Datenbasis).
4. Aspiration nur nach Eval-Refactor und mit δ≥100 cp + separatem Feature-Flag.

Dokument erstellt ohne Eingriff in Code oder bestehende Dateien. Alle Angaben basieren auf lesbarem Stand (git HEAD = 970d3bb + uncommitted Analyse-Dateien).

---
*Erstellt von MartuniBot-Analyse, 18.06.2026*