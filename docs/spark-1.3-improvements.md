# Spielstärke steigern — Ideensammlung (Spark 1.3)

**Stand:** 15.09.2026 · **Autor:** Muse-Code-Recherche (kein Engine-Code, nur Analyse)
**Quellen im Repo:** `src/search.rs`, `src/eval.rs`, `src/tt.rs`, `src/endgame.rs`,
`src/syzygy.rs`, `src/uci.rs`, `eval.toml`, `docs/roadmap.md`,
`docs/kimi-vorschlag.md`, `docs/ideen-umsetzungsanalyse-2026-06-18.md`,
`docs/performance-review.md`, `docs/lmr-plan.md`, `docs/see.md`.

**Leitprinzip (aus CLAUDE.md):** Engine-Logik ist Eigenleistung von Tobias.
Dieses Dokument kopiert keinen fremden Code und bindet nichts ein — es erklärt
Optionen, schätzt Aufwand/Nutzen/Risiko und lässt jede Entscheidung bei Tobias.
Alle Elo-Angaben sind Bandbreiten aus Engine-Erfahrungswerten und Martuni-Messungen,
keine Garantien. Verifikation läuft über den etablierten Prozess: Unit-Tests,
Node-Count-Bit-Exaktheit wo möglich, fastchess-SPRT, Lichess-Lookback.

---

## Kurzfassung — Top 7 nach Chance-Risiko-Verhältnis

| # | Hebel | Bereich | Erwartung | Aufwand |
|---|-------|---------|-----------|---------|
| 1 | Varianten-Gating verfeinern (SEE/Quiet-Checks, NMP/RFP je Variante) | Suche/Varianten | +1–2 Plies in KotH/3check/Horde/Racing Kings | mittel |
| 2 | Zeitmanagement: Soft/Hard-Limit, PV-Stabilität, Move-Komplexität | Uhr | +5–15 Elo Praxis | mittel |
| 3 | LMR Variante B (log-Formel) als A/B gegen Variante A | Suche | +5–15 Elo | klein–mittel |
| 4 | Futility Pruning (Zugebene) + Razoring als Ergänzung zu RFP | Suche | +5–10 Elo | mittel |
| 5 | Backward Pawns + Passed/Isolated-Tapering | Eval | +3–10 Elo | klein–mittel |
| 6 | Quiescence: Abzugsschach + selektive Quiet-Checks jenseits qply 0 | Suche | +3–8 Elo | mittel |
| 7 | Countermove-SPRT abschließen, dann Follow-up/History-Ausbau entscheiden | Ordering | Mess-Entscheid | klein |

Der Rest des Dokuments begründet diese Liste und nennt weitere Kandidaten.

---

## 1. Suche (Hauptpfad Standard/Chess960)

### S1. LMR Variante B als A/B gegen Variante A
- **Status quo:** Variante A, Stufenformel R=1 ab depth≥3 & Index≥3, R=2 ab
  depth≥6 & Index≥6, nur Non-PV, keine Captures/Promotions/Checks/Killer
  (`src/search.rs`: `lmr_reduction`, `LMR_MIN_DEPTH`).
- **Idee:** logarithmische Formel (Tiefe × Move-Index, Lookup-Tabelle) wie in
  `docs/lmr-plan.md` vorbereitet; Reduktion skaliert kontinuierlich statt in
  zwei Stufen. Optional: History-Score als Dämpfer (gute History → weniger R).
- **Erwartung:** +5–15 Elo typisch beim Umstieg Stufe→log; Haupteffekt ist mehr
  Tiefe bei gleichem Budget.
- **Risiko:** niedrig–mittel; Re-Search-Kaskade bleibt unverändert.
- **Verifikation:** fastchess-SPRT [0, 10], UHO, 1000+ Partien; Node-Counts auf
  Referenz-FENs als Sanity (dürfen sich ändern, Bestmoves sollten stabil bleiben).
- **Entscheid für Tobias:** erst reines A/B A-gegen-B, History-Kopplung separat.

### S2. LMR auch in PV-Knoten (konservativ)
- **Status quo:** `can_reduce` fordert `!is_pv`.
- **Idee:** in PV-Knoten reduzieren, aber mit kleinerem R (z. B. R−1, höherer
  Mindest-Index). Stockfish-Stil; nutzt, dass auch in der PV späte Züge selten
  widerlegen.
- **Erwartung:** +3–8 Elo, mehr Tiefe in ruhigen Stellungen.
- **Risiko:** mittel — PV-Verfälschung möglich, braucht sauberes SPRT.
- **Verifikation:** SPRT gegen S1-Baseline; W5AboGf0-Schlüsselstellung als Regression.

### S3. Futility Pruning auf Zugebene + Razoring (Ergänzung zu RFP)
- **Status quo:** nur RFP live (depth ≤ 3, Margin 120 cp/Tiefe, `rfp_cutoff`).
  Klassisches Futility (Zugebene am Blatt: static_eval + Zugwert + Margin < alpha
  → skip) und Razoring (depth 1–2: static_eval + großer Margin < alpha → reduzierte
  Suche statt Vollsuche) fehlen — beide stehen in Roadmap/Kimi als offen.
- **Idee:** zuerst Move-Futility an depth 1–2 für Quiet-Moves (keine Checks,
  keine Promotions, kein Matt-Score-Fenster); Razoring danach als separater Hebel.
- **Erwartung:** je +3–8 Elo, −20–40 % Knoten an Blättern.
- **Risiko:** mittel; Margins sind Tuning-Knöpfe (Off-Schalter-Konvention
  `MARTUNI_FP_OFF` / `MARTUNI_RZ_OFF` analog NMP/RFP beibehalten).
- **Verifikation:** SPRT; Blunder-Klasse `missed_capture` im Lookback beobachten
  (Futility darf Taktik nicht blind machen).

### S4. Late Move Pruning (LMP) / Move Count Pruning
- **Status quo:** nicht vorhanden — jeder legale Zug wird mindestens per Scout
  gesucht.
- **Idee:** an Blatt-nahen Knoten (depth ≤ 3–4) späte Quiet-Moves (Index jenseits
  einer depth-abhängigen Schranke, z. B. 3+depth²) ganz überspringen, wenn kein
  PV, kein Schach, kein Matt-Fenster, best_score nicht verzweifelt.
- **Erwartung:** +3–8 Elo durch Tiefengewinn; Standard-Hebel aller modernen Engines.
- **Risiko:** mittel; Schranke zu aggressiv kostet Taktik — konservativ starten.
- **Verifikation:** SPRT; getrennt von S3 messen (Interaktion!).

### S5. Adaptives NMP + Verification Search
- **Status quo:** R=2 konstant, Mindesttiefe 3, Zugzwang-Schutz via
  `has_non_pawn_material` (`NMP_REDUCTION`, `NMP_MIN_DEPTH`).
- **Idee (zwei Stufen):** (a) R = 2 + depth/6 (tief → stärker reduzieren);
  (b) Verification Search: nach NMP-Cutoff bei mittlerer Tiefe eine reduzierte
  Suche *mit* Zugzwang-Verdacht zur Bestätigung (klassisch gegen Zugzwang-Blindheit).
- **Erwartung:** (a) +2–6 Elo, fast gratis; (b) +0–3 Elo, v. a. weniger
  Endspiel-Ausreißer.
- **Risiko:** niedrig (a), mittel (b).
- **Verifikation:** (a) SPRT; (b) Endgame-Blunder-Rate im Lookback + SPRT.
- **Entscheid:** Roadmap sagt „erst wenn Endgame-Rate Anlass gibt" — aktuell
  kein Druck, also (a) vorziehen, (b) parken.

### S6. ProbCut
- **Status quo:** nicht vorhanden.
- **Idee:** an Knoten mittlerer Tiefe (depth ≥ 5, nicht PV, nicht im Schach,
  beta nicht Matt): wenn static_eval − Margin ≥ beta, eine flache Capture-Suche
  (Quiescence oder depth−4 mit engem Fenster beta−1/beta); schlägt sie hoch,
  Cutoff. Fängt grobe Fail-Highs billig ab.
- **Erwartung:** +3–8 Elo bei guter Margin-Wahl.
- **Risiko:** mittel; Margin-Tuning nötig.
- **Verifikation:** SPRT; als eigenständiger Hebel nach S3/S4 testen.

### S7. Singular Extensions / Multi-Cut
- **Status quo:** nicht vorhanden; Extensions sind Check (+1/+2), gewinnender
  Capture, erkanntes Endspiel, Freibauer (je +2, Cap 4 pro Linie).
- **Idee:** Singular Extension: ist der TT-Move deutlich besser als alle
  Alternativen (Verifikation mit reduzierter Suche ohne TT-Move), wird er
  verlängert. Multi-Cut: schlagen mehrere Züge in reduzierter Suche hoch,
  Cutoff ohne Vollsuche.
- **Erwartung:** Singular +5–15 Elo (größter einzelner Such-Hebel, der noch fehlt),
  Multi-Cut +2–5 Elo.
- **Risiko/Aufwand:** hoch — komplexeste Baustelle in dieser Liste, TT-Interaktion,
  Tuning (Margins, Tiefen). Kimi-Einschätzung „hoher Aufwand" gilt weiter.
- **Verifikation:** erst Prototyp mit Off-Schalter, dann langes SPRT (2000+ Partien),
  Taktik-Suite (z. B. W5AboGf0-Familie) als Regression.
- **Entscheid:** nur angehen, wenn S1–S6 ausgeschöpft sind oder ein Taktik-Defizit
  es erzwingt.

### S8. Extension-Politik überprüfen (Check +2, Kandidaten +2)
- **Status quo:** Schach im Endspiel +2, sonst +1; Nicht-Schach-Kandidaten
  (SEE≥0-Capture, erkanntes Endspiel, Freibauer) pauschal +2 bei Cap 4
  (`is_candidate_move`).
- **Idee:** A/B-Varianten: (a) Kandidaten nur +1; (b) Freibauer-Extension nur für
  weit vorgerückte Bauern (6./7. Reihe); (c) Capture-Extension nur für SEE deutlich
  > 0. Die +2-Politik ist aggressiv und stammt aus der Zeit vor LMR/RFP.
- **Erwartung:** ±5 Elo je Richtung — kann in beide Richtungen ausschlagen;
  Ziel ist Tiefen-Ökonomie.
- **Verifikation:** je Variante ein SPRT; Node-Counts auf Taktik-FENs.

### S9. SEE-Pruning staffeln statt binär
- **Status quo:** Hauptsuche depth ≤ 2: SEE<0-Captures skip (außer bestgeordneter
  Zug, Checks, Matt-Verzweiflung); Quiescence: SEE<0 skip + Delta-Pruning 150.
- **Idee:** Schwelle depth-abhängig (z. B. depth 1: SEE<0, depth 2–3: SEE<−50…−100),
  und Quiet-SEE (`see_quiet`) auch für LMR-Entscheid nutzen.
- **Erwartung:** +0–4 Elo, weniger Opfer-Blindheit (vgl. 29.05-Befund Nxf7-Familie).
- **Verifikation:** SPRT + `missed_capture`-Rate im Lookback.

### S10. Aspiration Windows — weiterhin parken
- **Status quo:** 16.05. verworfen (Re-Search-Quote 102 %, −2 Plies), Code entfernt.
- **Einschätzung:** kein neuer Versuch vor Eval-Stabilisierung; frühestens nach den
  Eval-Hebeln aus Abschnitt 6 mit δ≥100 und Feature-Flag erneut prüfen. Kein
  kurzfristiger Hebel.

## 2. Move Ordering

### M1. Countermove-SPRT abschließen (offen seit 01.09.)
- **Status quo:** Countermove live (Default an, `MARTUNI_CM_OFF=1`), SPRT [0, 10]
  nie gefahren.
- **Idee:** schlicht das überfällige SPRT fahren und danach behalten/verwerfen.
- **Erwartung:** Mess-Entscheid, kein Elo-Versprechen.
- **Entscheid:** vor allen weiteren Ordering-Hebeln, sonst vermischen sich Effekte.

### M2. History-Ausbau: Follow-up / Countermove-History
- **Status quo:** Killer (2/Ply), Countermove (1), klassische History [side][from][to].
- **Idee:** Countermove-History (History-Tabelle indiziert über Gegnerzug) oder
  Follow-up-History (eigener Vorzug) als Fein-Sortierung der Quiets; optional als
  LMR-Dämpfer (S1).
- **Erwartung:** +2–6 Elo.
- **Risiko:** niedrig–mittel; mehr Tabellen, mehr Tuning.
- **Verifikation:** SPRT gegen M1-Baseline.

### M3. SEE-Ergebnis für Ordering-Qualität nutzen
- **Status quo:** SEE sortiert Captures (MVV/LVA-Ordnung), `see_val` gecacht.
- **Idee:** (a) Killer/History-Quiets mit stark negativem Quiet-SEE nach hinten;
  (b) inkrementelles SEE (`all_attackers_to` cachen — Roadmap-Punkt) als NPS-Hebel.
- **Erwartung:** (a) +0–3 Elo, (b) +2–5 % NPS.
- **Verifikation:** (a) SPRT, (b) NPS-Benchmark best-of-N.

### M4. Antichess-Ordering invertieren
- **Status quo:** `variant_capture_value` bewertet jeden Schlag positiv —
  im Räuberschach ist Materialgewinn schlecht (Roadmap, Prüfer-Befund 05.09.).
- **Idee:** Antichess-eigenes Ordering (Schläge mit Materialverlust zuerst prüfen,
  da oft erzwungen/gut) oder eigenes Antichess-SEE mit umgekehrtem Vorzeichen.
- **Erwartung:** deutlich bessere Antichess-Tiefe und -Spielstärke (kein Standard-Effekt).
- **Verifikation:** Antichess-Selfplay + Tiefe/Zeit-Messung; Standard bit-exakt halten.

## 3. Quiescence Search

### Q1. Abzugsschach erkennen (v2 des Check-Gates)
- **Status quo:** nur direkte Quiet-Checks am Q-Eintritt (qply==0), Abzugschachs
  bewusst nicht erfasst (`quiescence`, Check-Masken).
- **Idee:** Abzugschach-Test über Linienöffnung (wegziehende Figur gibt Batterie frei)
  als Ergänzung der Direkt-Maske, weiter nur qply==0 und nur mit SEE-Gate.
- **Erwartung:** +2–5 Elo, bessere Mattnetz-Erkennung.
- **Risiko:** mittel — Explosionsgefahr ohne strenge Gates.
- **Verifikation:** SPRT + Matt-in-N-Suite.

### Q2. Selektive Quiet-Checks über qply 0 hinaus
- **Status quo:** Checks nur am Eintritt, danach nur Captures/Promotions.
- **Idee:** Checks bis qply 1–2 zulassen, aber nur mit Delta-/SEE-Gates und
  Stückwert-Schranke (z. B. nur Checks, die Material gewinnen oder Matt drohen).
- **Erwartung:** +2–5 Elo.
- **Risiko:** mittel–hoch (Q-Explosion); eng deckeln und messen.

### Q3. Recapture-Extension / Q-TT-Probe
- **Status quo:** QSearch schreibt/liest keine TT; keine Recapture-Sonderbehandlung.
- **Idee:** (a) Recapture (Schlagen auf dem Feld des letzten Tauschs) um 1 Ply
  verlängern; (b) TT-Probe in Q (nur Cutoff, kein Store — billig).
- **Erwartung:** (a) +0–3 Elo, (b) +1–3 % NPS-äquivalent durch Cutoffs.
- **Verifikation:** jeweils eigenes SPRT bzw. Node-Count-Vergleich.

### Q4. Delta-Margin und MAX_QPLY tunen
- **Status quo:** DELTA_MARGIN 150 (war 200), MAX_QPLY 12 (quiescence-relativ).
- **Idee:** A/B-Staffel für Margin (100/150/200) und Cap (10/12/16); kleinster
  Aufwand aller Q-Hebel.
- **Erwartung:** ±3 Elo — Tuning, kein Strukturgewinn.

## 4. Transposition Table

### T1. Bucket-/Cluster-TT (mehrere Slots pro Index)
- **Status quo:** 1 Slot pro Index, depth-preferred + Exact-Prio + Generation/Age
  (K=3) — Stand nach 24.06.
- **Idee:** 2–4 Slots pro Bucket (always-replace + depth-preferred getrennt);
  klassischer Hitraten-Hebel.
- **Erwartung:** +3–8 Elo durch stabilere Cutoffs und besseres Ordering.
- **Risiko/Aufwand:** mittel; mehr Speicher-Logik, aber gut testbar.
- **Verifikation:** TT-Hitrate instrumentieren (Debug-Zähler), dann SPRT.

### T2. TT-Score als Eval-Hint (TT-cutoff light)
- **Status quo:** TT liefert Cutoff oder Move-Hint; kein Eval-Seeding.
- **Idee:** TT-Eval als Startwert für RFP/NMP/Pruning-Margins nutzen (statt frischer
  Eval), wo zulässig — spart Eval-Aufrufe an besuchten Knoten.
- **Erwartung:** +1–3 % NPS-äquivalent.
- **Verifikation:** Node-Count-/NPS-Vergleich, SPRT als Absicherung.

### T3. Kein Handlungsbedarf: Mutex, Mate-Normierung
- TT-Lock-once ist drin, Mate-Normierung (ply-adjust) korrekt — die alten
  Review-Punkte sind erledigt.

## 5. Zeitmanagement (Z)

### Z1. Soft-/Hard-Limit mit PV-Stabilität (größter Uhren-Hebel)
- **Status quo:** `remaining/30 + 0.8·inc`, Overhead-Deckel, kein Unterschied ob
  die PV stabil ist oder springt (`calculate_think_time`).
- **Idee:** Soft-Limit (bei stabiler PV früh stoppen und Zeit sparen) + Hard-Limit
  (niemals überschreiten); Verlängerung bei: PV-Wechsel in letzter Iteration,
  Score-Drop > Schranke, nur ein legaler Zug → sofort ziehen (Forced-Move spart
  schon, aber auch „praktisch forciert" erkennen).
- **Erwartung:** +5–15 Elo in der Praxis (Zeitnot-Fehler runter, Crit-Momente
  tiefer gerechnet) — wirkt auf jede Partie.
- **Risiko:** niedrig–mittel; mit Zeit-Suites + Lichess-Lookback (Zeitnot-Quote)
  absichern. Flag-Zähler im Blick: nie aggressiver starten als heute.
- **Verifikation:** Bullet/Blitz-SPRT (dort schlägt Uhr am stärksten durch) +
  Lookback Zeitnot-Rate.

### Z2. Phasen-/Komplexitäts-abhängige Zeit
- **Status quo:** flache 1/30-Regel, keine Phasen- oder Stellungs-Komplexität.
- **Idee:** mehr Zeit im Mittelspiel/bei vielen legalen Zügen/offener Stellung,
  weniger im theoretischen Endspiel und bei Buch-nahen Stellungen; Inkrement-Anteil
  nach Bedenkzeit-Rest staffeln.
- **Erwartung:** +2–6 Elo zusätzlich zu Z1.
- **Verifikation:** gemeinsam mit Z1 im SPRT, aber getrennt schaltbar.

### Z3. Ponder-Trefferquote erhöhen
- **Status quo:** Ponder mit TT-Pondermove funktioniert.
- **Idee:** Ponder-Hit-Rate loggen (wie oft trifft der Pondermove?); bei niedriger
  Quote Ponder-Suche breiter anlegen (TT-Move + 1–2 Alternativen andeuten ist
  nicht möglich single-threaded — aber die Ponder-Zeit anders gewichten).
- **Erwartung:** unklar bis zur Messung — erst instrumentieren.

## 6. Evaluation (Standard/Chess960)

### E1. Backward Pawns (letzter klassischer Bauernstruktur-Term)
- **Status quo:** isolierte Bauern (−20 flach), Freibauern per Rang, Phalanx,
  Outposts — rückständige Bauern fehlen (Roadmap + Kimi einig).
- **Idee:** Strafe für Bauern ohne Nachbarbauern hinter sich, deren Vorrückfeld
  sicher gegnerisch kontrolliert ist; MG/EG-getapert, in `eval.toml` einstellbar.
- **Erwartung:** +3–8 Elo.
- **Verifikation:** SPRT; Eval-Breakdown-Sanity auf Referenzstellungen.

### E2. Tapering für Passed/Isolated fertigstellen
- **Status quo:** Passed-Bonus per Rang ohne MG/EG-Split, isolierte Bauern
  phasenflach (Roadmap-Punkte 2–3 in `eval-kalibrierung.md`).
- **Idee:** beide Terme MG/EG-tapern (Freibauern im EG mehr wert, Isolanis im EG
  schwächer/stärker je nach Deckung — Messung entscheidet Richtung).
- **Erwartung:** +2–6 Elo.
- **Verifikation:** SPRT, getrennt von E1.

### E3. Hängende Figuren / Bedrohungen verallgemeinern
- **Status quo:** `heavy_piece_threat` binär; kein allgemeiner „unverteidigter
  Angriff auf höherwertigen Stein"-Term (Kimi: teilweise).
- **Idee:** Threat-Eval: für jede Figur Abzug, wenn sie angegriffen und unzureichend
  verteidigt ist, gewichtet mit Stückwert-Differenz; simpel starten (nur Minor/Major),
  dann verfeinern.
- **Erwartung:** +3–8 Elo, weniger `hangs_*`-Blunder im Lookback.
- **Verifikation:** SPRT + `hangs_bishop/knight`-Rate im Lookback.

### E4. King Safety: Pawn-Storm / Flügelangriff
- **Status quo:** 3×3-Zone, Angreifer-Gewichte, SafetyTable, Pawn Shield — kein
  Sturm-Bonus (eigene Bauern vorm Gegner-König), keine Rochaden-Asymmetrie.
- **Idee:** Bonus für vorgerückte Sturm-Bauern bei entgegengesetzten Rochaden;
  Malus für offene Sturm-Linien vorm eigenen König.
- **Erwartung:** +2–6 Elo, v. a. schärfere Angriffspartien.
- **Verifikation:** SPRT; Lookback `exposed_king`/`allows_mate`.

### E5. Space / Raumvorteil
- **Status quo:** fehlt (Outposts drin, Space offen).
- **Idee:** einfacher Space-Term (eigene Bauern-/Figuren-Präsenz in Zentrum und
  gegnerischer Hälfte, phasenbeschränkt aufs Mittelspiel).
- **Erwartung:** +0–4 Elo.
- **Verifikation:** SPRT.

### E6. Bishop-Trap, Pawn-Shield-vorne-König, Score-Struct
- Kleine Roadmap-Restanten: Läufer-Fallen-Erkennung, Shield bei vorgerücktem König,
  `(i32,i32)`→`Score`-Struct-Refactor. Je +0–2 Elo bzw. Code-Qualität — als
  Füllarbeit zwischen den großen Hebeln, nicht als Priorität.

### E7. 960-Eröffnungsdrift: Hebel #2 (eskalierender Rochade-Malus)
- **Status quo:** 960-Paket live (Bishop-Backrank, Uncastled-Flank, flat
  `castle_rights_mg` −15); Rochadequote 46→56 %, aber Ø Rochadezug 19 weiter
  spät; ply≤16-Blunder in 960 zehnmal häufiger (Roadmap „Nächste Schritte" 4).
- **Idee:** Malus mit Zugzähler wachsen lassen statt flat.
- **Erwartung:** 960-spezifisch +5–15 Elo (FRC-SPRT hat beim Vorgänger +28 gezeigt).
- **Verifikation:** FRC-A/B (alle 960 Stellungen) + Standard-Gate (regressionsfrei).

## 7. Endspiel & Syzygy (EN)

### EN1. Endspielwissen 6–14 Steine (oberhalb Syzygy)
- **Status quo:** Syzygy ≤ N Steine + Mop-up/KPK/KBNK + Pawn-Endgame-Guard;
  dazwischen generische Eval. Review 18.06.: „Gros der Blunder in 8–14 Steinen".
- **Idee:** gezielte Heuristiken: Turmendspiele (Turm hinter Freibauern — teilweise
  da, Aktivität, Lucena/Philidor-Muster light), Läufer-/Springer-Minderheit,
  Festungs-Erkennung light (falscher Läufer + Randbauer ist Anfang, nicht Ende).
- **Erwartung:** +5–15 Elo + bessere Konversion; größter Eval-Block nach den Bauern.
- **Verifikation:** Endspiel-Suite + SPRT + `trade_down`/Endgame-Blunder-Rate.

### EN2. Syzygy: WDL am Blatt / DTZ-Folgeprüfung
- **Status quo:** WDL-Probe an inneren Knoten, DTZ an der Wurzel.
- **Idee:** (a) WDL auch zur QSearch-Blatt-Korrektur (billig, wenn Steine ≤ N);
  (b) 50-Züge-Sicherheit der gewählten Linie prüfen (nicht nur Wurzelzug).
- **Erwartung:** +0–3 Elo, weniger Remis-Drift in gewonnenen Tablebase-Stellungen.
- **Verifikation:** Tablebase-Test-Suite + Lookback 50-Züge-Remis-Quote.

## 8. Varianten (V)

### V1. Feines Search-Gating je Variante (Roadmap-Punkt 2 — dringendster Varianten-Hebel)
- **Status quo:** `uses_standard_rules()==false` schaltet pauschal NMP/RFP/Quiet-Check-Q/orthodoxes-SEE ab; Folge: KotH/3check Tiefe 5 in 3 s vs. Standard Tiefe 7 in 0,5 s.
- **Idee (zwei Stufen, aus Roadmap):** (a) SEE/Quiet-Checks für KotH, 3check,
  Horde, Racing Kings wieder an (gleiche Schlag-/Schach-Mechanik, bit-exakt
  prüfbar); (b) NMP/RFP für KotH und 3check wieder an (Racing Kings ohne NMP —
  Nullzug verschiebt das Rennen; Horde-Weiß ohne König — Zugzwang-Schutz greift
  nicht; Antichess bleibt eigene Welt).
- **Erwartung:** +1–2 Plies in den vier Varianten ≈ großer Elo-Sprung dort.
- **Verifikation:** Varianten-Selfplay (fastchess kann `variant`; UHO-Buch nur
  für KotH/3check sinnvoll), Tiefe/Zeit-Messung, Standard bit-exakt.
- **Entscheid:** Stufe (a) sofort, (b) nach Messung.

### V2. Varianten-Evals kalibrieren und nach eval.toml holen
- **Status quo:** Module in `src/variants/` sind Erstentwürfe, Konstanten im Code,
  kein A/B (Roadmap).
- **Idee:** Konstanten nach `eval.toml` (eigene Sektionen), dann je Variante
  Mini-SPRT oder gezielte Stellungs-Suiten; Horde-+48-Statik (entschiedene Stellung
  zeigt cp statt Verlust) als Füllfix mitnehmen — harmlos, weil die Suche vorher
  terminal abschneidet.
- **Erwartung:** je Variante +5–20 Elo (Startniveau niedrig, Hebel groß).
- **Verifikation:** Varianten-Selfplay; Grok-Schleuse-Lookback sobald Material da.

### V3. Varianten-Adapter-NPS (linearer Lookup, Lazy-Listen)
- **Status quo:** PR #4 hat Atomic/Crazyhouse/960 beschleunigt; BoardShak baut
  Listen/Eager-Outcomes weiter pro Knoten; lineare Lookups in `make_move_new`/`is_capture`.
- **Idee:** Index über from/to statt linearer Suche; Lazy-Listen auch für BoardShak
  (Antichess-Terminal-Optimierung erhalten).
- **Erwartung:** +5–15 % Varianten-NPS.
- **Verifikation:** NPS-Benchmark je Variante; Perft-Regression gegen shakmaty/python-chess.
- **Reihenfolge:** erst V1 (Gating wiegt mehr), dann V3.

### V4. Antichess-Sonderpunkte
- M4 (Ordering) + Remis-durch-Material-Erkennung (ungleichfarbige Läufer, die sich
  nie schlagen können) + `ANTICHESS_FORCED_QPLY_MAX`-Tuning (4 vs. 6/8 im
  Antichess-A/B). Alles Standard-unberührt.

## 9. Eröffnung & Buch (B)

### B1. Buchauswahl diversifizieren
- **Status quo:** Polyglot-Set + Patch-Buch, laufend gepflegt; Auswahlmechanismus
  simpel.
- **Idee:** gewichtete Zufallsauswahl nach Buch-Gewichten statt deterministisch
  (mehr Abwechslung, weniger Wiederholungs-Blunder gegen gleiche Gegner);
  Patch-Workflow (`build_book_patches.py`) beibehalten.
- **Erwartung:** +0–5 Elo Praxis (weniger Prep-Treffer), v. a. Anti-Sparring-Wert.
- **Verifikation:** Lookback Wiederholungs-Blunder (`stickshark99`-Familie).

### B2. Buchlücken-Monitoring automatisieren
- **Status quo:** manuell aus dem Lookback.
- **Idee:** Cron-Report „Stellung X kam N×, Score Y" aus den Analyse-JSONs als
  Patch-Vorschlag — kein Engine-Code, nur Tooling.
- **Erwartung:** Prozessgewinn, kein direkter Elo.

## 10. Performance & Infrastruktur (P)

### P1. Profiling vor Optimierung
- **Status quo:** NPS Standard ~2,3 M; Varianten 0,5–1,9 M (Stand 11.09.).
- **Idee:** einmal `perf`/flamegraph auf Release-Build an 2–3 Referenz-FENs;
  die Top-3 Hotspots entscheiden die nächsten P-Hebel (Kandidaten: Eval-Kontext
  pro Blatt, `all_attackers_to`, Varianten-Adapter).
- **Erwartung:** unbekannt bis zur Messung — aber jede Optimierung ohne Profil
  ist Raten.

### P2. Eval-Kontext-Caching / inkrementelle Eval
- **Status quo:** volle Eval pro Blatt (Review-Befund 18.06.).
- **Idee:** erst bit-exaktes Caching (Kontext einmal pro Knoten), später
  inkrementell (Material/PST-Deltas). Zweiteres ist ein Großprojekt.
- **Erwartung:** Caching +3–8 % NPS; inkrementell +10–25 %, aber hohes Risiko.
- **Verifikation:** NPS-Benchmark + bit-exakte Node-Counts (Caching-Stufe).

### P3. Kein SMP (kein Multi-Thread-Search)
- **Bewusst nicht vorgeschlagen:** Lazy-SMP wäre +30–60 Elo auf Mehrkern, aber
  ein Architektur-Umbau (TT-Locking, Thread-Pool, Determinismus-Verlust im Test).
  Erst wenn alle Single-Thread-Hebel ausgeschöpft sind. Hier nur als Merkposten.

## 11. Messmethodik (wie jeder Hebel zu verifizieren ist)

1. **Unit-Tests zuerst:** neue Logik mit Off-Schalter (`MARTUNI_*_OFF`-Konvention),
   Tests grün, Clippy sauber.
2. **Bit-Exaktheit wo versprochen:** Node-Counts/PVs auf 3–4 Standard-FENs
   (`go depth 8/9`) gegen Baseline-Binary — Pflicht bei Refactors, bei Such-Hebeln
   dokumentierte Abweichung.
3. **fastchess-SPRT [0, 10]** (Standard-Setup: UHO, 5+0.05, Hash 64 MB) als
   Rollout-Gate; FRC-Matches über alle 960 Stellungen, Varianten-Matches mit
   `variant`-Harness.
4. **Lichess-Lookback:** nach Rollout ~100 Partien gegen Anker (aktuell: 960 1720,
   Blitz 2159, Rapid 2290 — Stand 11.08., vor neuem Hebel aktualisieren);
   Blunder-Profil (`analyse-*.json` via Grok-Schleuse), Zeitnot-Quote, Rating.
5. **Ein Hebel pro Match:** keine Bündel — sonst ist bei Grün/Rot unklar, was trug
   (Lehre aus PR #4: drei Commits, ein Rollout, A/B zurückgestellt — nur nachholen,
   falls Spielstärke sichtbar nachlässt).

## 12. Empfohlene Reihenfolge (drei Staffeln)

**Staffel 1 — Mess-Schulden + billige Elo (Tage):**
M1 (Countermove-SPRT) → Q4 (Delta/QPLY-Tuning) → S5a (adaptives NMP) →
S8-Light (Freibauer-Extension nur 6./7. Reihe als Einzel-SPRT).

**Staffel 2 — Struktur-Hebel Suche + Uhr (Wochen):**
S1 (LMR-B) → S3 (Futility) → S4 (LMP) → S2 (PV-LMR) → Z1+Z2 (Zeitmanagement) →
T1 (Bucket-TT).

**Staffel 3 — Eval + Varianten + Endspiel (Wochen–Monate):**
E1 → E2 → E3 → E4 → EN1 → V1 → V2 → M4/V4 → E7 (960-Malus) → S6 (ProbCut) →
S7 (Singular, nur bei Bedarf).

Jede Staffel endet mit einem Lichess-Lookback; Staffel 2 zusätzlich mit einem
Bullet-Schwerpunkt (Uhr-Hebel zeigen dort am stärksten).

---

*Offen für Tobias: Reihenfolge bestätigen oder umstellen? Insbesondere:*
- *Z1 (Uhr) vorziehen — wirkt auf jede Partie, aber fasst die sensibelste Stelle an?*
- *S7 (Singular) überhaupt auf die Liste — oder als zu aufwendig streichen?*
- *P3 (SMP) als Fernziel aufnehmen oder explizit ablehnen?*
