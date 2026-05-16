# Dynamische Figurenbewertung — Schritt 3 (Bishop-Pair)

**Status:** Konzept, Entscheidung offen. Kein Code geschrieben.
**Erwarteter Elo-Gewinn:** +0 bis +15 Elo. Eval-Hebel, kein Tiefen-
Mechanik. Analog zu Step 1 / Step 2 ist ein knappes A/B-Signal
plausibel — Lichess-Lookback wird das definitive Verdikt liefern.
**Voraussetzung:** Step 1 + Step 2 v2 laufen stabil ([[project-
auswertung-2026-05-15]] hat das bestätigt, Lichess-Lookback flach
ohne Trigger).
**Betroffene Module:** `src/eval_config.rs` (neue Felder), `src/eval.rs`
(Logik in `evaluate_side` und `build_breakdown`), `eval.toml` (neue
Werte). **Keine** Änderung an `search.rs` / `endgame.rs` / `position.rs`.

## Was hat Step 3 zu leisten

Aktueller Stand: `bishop_pair_each = 15` → bei zwei Läufern werden
**konstant 30 cp** addiert, unabhängig von Phase und Bauern-Struktur
(`evaluate_side`, eval.rs:128-130).

Theorie (Kaufman 1999, von Tobias im Master-Prompt geforderter
Maßstab — `docs/vorbereiteter_Prompt_dynamische_Figurenbewertung.md`):
- Im **Mittelspiel** ist das Läuferpaar moderat wertvoll (~22–30 cp).
- Im **Endspiel** wächst der Bonus deutlich, weil Läufer ihre
  Reichweite ohne Tempoverlust nutzen können.
- Zusätzlich wirkt **Brett-Offenheit**: weniger Bauern auf dem Brett
  → mehr freie Diagonalen → Bishop-Pair stärker.

Step 3 macht aus dem statischen 30-cp-Block einen
**phasen-getaperten Term mit Offenheits-Skala**:

```
mg_bp = bishop_pair_mg
eg_bp = bishop_pair_eg + (16 - total_pawn_count) * bp_open_scale
bonus = taper(mg_bp, eg_bp, phase)        // bestehende taper-Funktion in eval.rs:51
```

`total_pawn_count` zählt alle Bauern (beide Seiten) — konsistent mit
der Läufer-Logik aus Step 2 (siehe `piece_material`, eval.rs:392).

## Implementierungs-Skizze

### `src/eval_config.rs`

```rust
// Schritt 3 der dynamischen Figurenbewertung (Bishop-Pair).
// taper(mg, eg + (16 - total_pawns) * open_scale, phase) ersetzt
// das statische `2 * bishop_pair_each`. Defaults sind so gewählt,
// dass ohne TOML-Override exakt das Step-2-Verhalten (= 30 cp
// statisch) erhalten bleibt: 30 / 30 / 0.
pub bishop_pair_mg: i32,
pub bishop_pair_eg: i32,
pub bp_open_scale: i32,
```

Default-Werte: `bishop_pair_mg = 30, bishop_pair_eg = 30, bp_open_scale = 0`
→ identisch zu altem `2 * bishop_pair_each` mit `bishop_pair_each = 15`.

Loader-Eintrag analog zu den existierenden in `[pieces]` oder einer
neuen `[material_dynamic]`-Sektion — siehe Entscheidung 2 unten.

### `src/eval.rs`

```rust
// Stelle eval.rs:127-130 ersetzen:
let our_bishops = *board.pieces(Piece::Bishop) & our_bb;
if our_bishops.popcnt() >= 2 {
    let eg_value = p.bishop_pair_eg + (16 - total_pawn_count) * p.bp_open_scale;
    score += taper(p.bishop_pair_mg, eg_value, phase);
}
```

`total_pawn_count` ist bereits in `evaluate_side` berechnet (Zeile 103).
`taper` ist eval.rs:51 vorhanden — exakt dieselbe Logik wie Dynmat
Step 1 für N/B-Material.

`build_breakdown` (eval.rs:921) analog:
```rust
let eg_value = p.bishop_pair_eg + (16 - total_pawn_count) * p.bp_open_scale;
b.bishop_pair = taper(p.bishop_pair_mg, eg_value, phase);
```

### `eval.toml`

```toml
# Bishop-Pair (Schritt 3 der dynamischen Figurenbewertung, 16.05.2026)
# - bishop_pair_mg: Bonus im Mittelspiel (Σ für das Paar, nicht pro Läufer)
# - bishop_pair_eg: Basis-Bonus im Endspiel (Σ)
# - bp_open_scale:  zusätzlicher cp pro fehlendem Bauer auf dem Brett (16 - total)
# Defaults im Code: 30 / 30 / 0 → identisch zum alten `2 * bishop_pair_each`.
# Defaults in eval.toml siehe Variante A / B / C unten.
bishop_pair_mg = ?
bishop_pair_eg = ?
bp_open_scale  = ?
```

Das alte Feld `bishop_pair_each = 15` bleibt **vorerst im Code und
TOML** als Anker (analog `p.knight`/`p.bishop` bei Step 1) — wird in
der Eval-Logik nicht mehr verwendet. Aufräumen erst, wenn Step 3
bestätigt ist und keine externen Skripte mehr darauf verweisen.

## Drei Varianten

Reihenfolge: konservativ → aggressiv. Tobias entscheidet, ich baue
genau eine davon.

### Variante A — Tobias' Original (Empfehlung)

```
bishop_pair_mg = 30    # = aktueller Wert, MG-Verhalten unverändert
bishop_pair_eg = 50    # +20 cp im EG (volles Brett)
bp_open_scale  = 2     # max +32 cp bei leerem Brett (16 - 0 = 16, × 2)
```

Im EG mit allen 16 Bauern: 50 cp. Im EG mit nur 6 Bauern: 50 + 10×2 = 70 cp.
Im MG: weiter konstant 30 cp.

Begründung: entspricht der Vorgabe aus
`docs/vorbereiteter_Prompt_dynamische_Figurenbewertung.md` und ist der
einzige Spread-Wert, den Tobias dort explizit kalibriert hat.

### Variante B — Kaufman-näher (Stockfish-Region)

```
bishop_pair_mg = 22
bishop_pair_eg = 50
bp_open_scale  = 3
```

MG leicht reduziert (Kaufman empfiehlt ~11 cp/Läufer im MG), EG mit
stärkerer Offenheits-Skala. Mehr Spread, aber größerer Eingriff im
Mittelspiel.

Risiko: **MG-Wert sinkt von 30 auf 22** — andere MG-Terme (King-Safety,
PST-Tapering) sind auf die alte Skala kalibriert. Bei Step 1 / Step 2
war Tobias explizit: MG-Anker nicht antasten ohne A/B.

### Variante C — minimaler Hebel

```
bishop_pair_mg = 30
bishop_pair_eg = 40
bp_open_scale  = 1
```

Kleinster Sprung weg vom Status Quo. Im EG max 40 + 16×1 = 56 cp (bei
leerem Brett). Praktisch kaum unterscheidbar vom aktuellen Wert bei
gefüllten Brettern.

Risiko: zu schwach für ein klares A/B-Signal — kann genau wie Step 2
v1 (Pawn-Scales 3/4) in einem nicht-signifikanten Match enden, dann
ist Variante A ohnehin die nächste Iteration.

## Eval-Anker-Frage (analog Step 1)

In Step 1 hatte Tobias festgelegt: `p.knight` / `p.bishop` bleiben als
statische Anker für `king_exposure_penalty` und `endgame::strong_material`
unangetastet. Step 3 ist symmetrisch — sollte `bishop_pair_each = 15`
im Code/TOML als toter Wert bleiben (Anker-Disziplin, keine externe
Logik berührt), oder direkt entfernt werden (Cleanup, weil keine
externe Referenz vorhanden ist)?

Stand: `bishop_pair_each` wird **nirgendwo** außerhalb von eval.rs/
eval_config.rs referenziert (per grep verifiziert, 16.05.2026). Anker
hat keine technische Notwendigkeit, ist nur Konsistenz mit dem
Step-1-Pattern.

## Test-Plan

Standard analog Step 1 / Step 2 v2:

1. **Smoke-Test.** Ein 9-Stellungen-Smoke wie Cluster-1b hilft hier
   wenig — Bishop-Pair wirkt strukturell, nicht taktisch. **Lokaler
   Eval-Sanity:** zwei Stellungen mit Läuferpaar einmal MG/voller
   Brett, einmal EG/halb leeres Brett über `eval`-Kommando (Debug-
   Breakdown). Erwartung: MG-Bonus = neuer mg-Wert, EG-Bonus =
   neuer eg-Wert + Offenheits-Term. Direkter Reality-Check der
   Implementierung.
2. **A/B-Match.** Standard `~/tools/fastchess` SPRT [0, 10],
   1000 Partien 5+0.05 UHO_Lichess_4852_v1, Hash 64 MB, concurrency 2.
   Zwei Binaries: `martuni.backup-pre-step3-20260516` als Baseline,
   `martuni-dynmat-step3` als Challenger.
3. **Rollout-Entscheidung (während Tobias verreist).**
   **Step-2-v2-Disziplin** ([[feedback-ab-vs-lichess-signal]],
   Tobias' Vorgabe heute): LOS > 70 % UND CI klar auf positiver
   Seite → Rollout mit Backup-Binary. Sonst Engine bleibt Baseline,
   Match-Ergebnis wartet auf Rückkehr.
4. **Lichess-Lookback** nach Rollout: 100–150 Partien.
   Rollback-Trigger: Rating-Drop >30 Punkte gegen Stand 16.05.
   (Blitz 2039 / Rapid 2100) ODER `hangs_bishop`-Rate verschlechtert
   sich klar.

## Drei offene Entscheidungen

1. **Welche Variante (A / B / C)?** Empfehlung A (Tobias' Original).
   B berührt MG-Anker, das war bei Step 1 / 2 ausdrücklich nicht
   gewollt. C ist zu kleinschrittig.
2. **Anker-Disziplin**: `bishop_pair_each = 15` als toten Wert
   stehen lassen (Konsistenz mit Step-1-Pattern), oder direkt
   entfernen (Cleanup, keine externe Referenz)? Empfehlung: stehen
   lassen, analog `p.knight`/`p.bishop` aus Step 1 — kostet nichts
   und hält die Symmetrie.
3. **TOML-Sektion**: `bishop_pair_mg/eg/bp_open_scale` unter dem
   bestehenden `[pieces]`-Block, oder unter `[material_dynamic]`
   (wo schon `knight_mg/eg`, `bishop_mg/eg` liegen)? Letzteres ist
   semantisch sauberer (alle dynamischen Material-Felder beisammen),
   ersteres ist näher am Verwendungsort. Empfehlung: `[material_dynamic]`.
