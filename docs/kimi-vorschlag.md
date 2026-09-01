# Möglichkeiten zur Steigerung der Spielstärke von Martuni

Gesammelte Ideen auf Basis von `CLAUDE.md`, `README.md` und `docs/roadmap.md`.

**Stand: 01.09.2026** — Status je Punkt aktualisiert nach Abgleich mit dem
Code. Legende: **[done]** umgesetzt, **[verworfen]** gemessen & negativ,
**[offen]** noch nicht angegangen, **[teilweise]** Grundlage da, Ausbau offen.

## 1. Suche effizienter machen

- **[verworfen] Aspiration Windows** – Implementiert und 16.05.2026
  smoke-getestet: Re-Search-Quote 102 %, −2 Plies Gesamttiefe, Regression auf
  der Schlüsselstellung W5AboGf0. Befund in `docs/aspiration-windows.md`.
- **[teilweise] Adaptive Null-Move-Pruning** – NMP ist drin, aber mit
  konstantem `R = 2`; adaptive Variante (`R = 2 + depth/6`) und Verification
  Search stehen weiter in der Roadmap (aktuell „kein Druck").
- **[done] Static Exchange Evaluation (SEE)** – Vollständig in `src/search.rs`:
  Ordering gewinnender/verlierender Captures, SEE-Pruning in Hauptsuche
  (depth ≤ 2) und Quiescence, `see_quiet` für Schachzüge. Siehe `docs/see.md`.
- **[done] History Heuristic / Countermove Heuristic** – History seit langem
  drin; **Countermove am 01.09.2026 umgesetzt**: Tabelle `[side][from][to]`
  über den Gegnerzug indiziert, Cutoff-Quiets werden eingetragen, im
  MovePicker als Stufe 5 direkt hinter den Killers sortiert. Off-Schalter
  `MARTUNI_CM_OFF=1` (Konvention wie NMP/RFP). A/B-Match (SPRT) steht aus.
- **[teilweise] Futility Pruning / Razoring / Reverse Futility** – RFP seit
  16.08.2026 live (depth ≤ 3, Margin 120 cp/Tiefe). Klassisches
  Futility-Pruning am Blatt und Razoring weiter offen.
- **[offen] Singular Extensions / Multi-Cut** – Selektive Erweiterungen in
  offensichtlich einzigen Zugstellungen. Hoher Aufwand, bisher nicht angefasst.

## 2. Bewertungsfunktion ausbauen

- **[done] Dynamische Figurenwerte** – Dynmat Step 1–3 live, inkl.
  phasen-getapertem Läuferpaar-Bonus mit Offenheits-Skala
  (`bishop_pair_mg/eg`, `bp_open_scale`) und Low-Mobility-Staffel-Malus.
- **[teilweise] Pawn-Structure-Terme** – Isolierte und Freibauern (per Rang)
  sind drin; **rückständige Bauern (Backward Pawns) offen** (Roadmap).
- **[done] Feinere Mobilität** – Pro Figurentyp getrennte MG/EG-Mobilität auf
  Safe-Squares (`mobility_score` in `src/eval.rs`).
- **[teilweise] Besseres King Safety** – Pawn-Shield drin; Pawn-Storm und
  Flügelangriffe fehlen.
- **[teilweise] Bedrohungen / Hängende Figuren** – `heavy_piece_threat` deckt
  einen binären Fall ab; allgemeine „hängende Figur / Angriff auf
  höherwertigen Stein"-Terme fehlen.
- **[teilweise] Raumvorteil / Outposts** – Springer-Outposts drin; Raumvorteil
  (Space) fehlt.

## 3. Zeitmanagement und Ausrüstung

- **[offen] Bessere Zeitverteilung** – Immer noch einfaches
  `remaining/30 + 0.8·inc` mit Overhead-Deckel (`calculate_think_time` in
  `src/search.rs`). Keine Phasen-/Komplexitäts-Abhängigkeit.
- **[teilweise] TT stärker nutzen** – TT-Move-Ordering und Ponder-Move aus TT
  drin; NMP-Verifikation via TT offen.
- **[teilweise] Opening-Book verbessern** – Polyglot-Set plus Patch-Buch
  (`tools/build_book_patches.py`) werden laufend gepflegt; diverser
  Zugauswahlmechanismus offen.

## 4. Mess- und Test-Infrastruktur konsequent nutzen

- **[done, als Prozess etabliert]** A/B-Tests via fastchess-SPRT
  (`matches/`), Smoke-Tests vor Rollout, `docs/blunder-analyse.md`-Toolchain
  mit Cluster-Klassifikation und Lichess-Lookbacks. Konvention: Off-Schalter
  per Env-Var (`MARTUNI_NMP_OFF`, `MARTUNI_RFP_OFF`, `MARTUNI_CM_OFF`).

## Kurzfristige Empfehlung (Stand 01.09.2026)

Die ursprünglichen drei Empfehlungen sind erledigt (Aspiration verworfen,
SEE und dynamische Figurenwerte live). Als nächste Hebel mit gutem
Chance-Risiko-Verhältnis bleiben:

1. **Countermove Heuristic** – heute umgesetzt, SPRT-A/B als nächster Schritt.
2. **Backward Pawns** (Eval) – der letzte klassische Pawn-Struktur-Term,
   der noch fehlt; Roadmap-führend.
3. **Adaptive NMP** (`R = 2 + depth/6`) – kleiner, sicherer Such-Hebel, sobald
   die Endgame-Rate Anlass gibt.
4. **Zeitmanagement** – einmalige Investition, wirkt auf jede Partie.

Singular Extensions sind der größte verbleibende Such-Hebel, aber mit
deutlich höherem Implementierungs- und Verifikationsaufwand.
