# Pawn-Endgame-Guard — Konzept für Martuni

**Status:** Konzept-Phase, ausgelöst durch [[project-stickshark99-deepdive-2026-05-23]].
**Variante:** B (Eval-Term in `eval.rs`, additiv zu den bestehenden EG-Termen).
**Betroffene Module:** primär `src/eval.rs`, optional kleine Erweiterung in
`src/endgame.rs`, neue Parameter in `eval.toml`.

> **Warum jetzt:** Der 23.05.-Deep-Dive der 42 stickshark99-Partien zeigt
> einen klaren Cluster „K-Wanderung im KNP-Endspiel" (`MH3BeAfV` ply 91/105,
> `HxAG2UV8` ply 129) — Stellungen mit jeweils 1 Springer pro Seite und
> wenigen Bauern, in denen Martuni mit dem König geometrisch falsch
> manövriert. Ein strict-KPK-Term (Variante A aus der Konzept-Diskussion)
> würde diese Stellungen gar nicht erkennen, weil die Material-Signatur
> nicht KPK ist. Variante B ergänzt deshalb die bestehenden EG-Terme
> in `eval.rs` um Opposition-, Key-Square- und Rook-Pawn-Wissen, **gated
> auf Stellungen mit geringem Offizier-Material**.

## Idee in einem Satz

In Endspielen, in denen das Material so gering ist, dass König-Manöver
spielentscheidend werden, soll die Eval die wichtigsten Lehrbuch-Konzepte
explizit kennen (Opposition, Schlüsselfelder vor eigenen Freibauern,
Rook-Pawn-Sonderfall), damit die Suche nicht erst auf hoher Tiefe darauf
stoßen muss.

## Was schon greift — Bestandsaufnahme

| Term | Datei | Wirkung |
|---|---|---|
| `endgame::endgame_score` (Signatur KPK) | `endgame.rs:186` | Strict KPK: erkennt Rule of the Square via `is_pawn_unstoppable`. Bei „Bauer im Quadrat" → `None`, normale Eval übernimmt. |
| `king_activity_endgame` | `eval.rs:31` | Pusht den eigenen König im EG ins Zentrum (Phase-gated). |
| `king_passed_pawn_synergy` | `eval.rs:699` | Bonus für König nahe eigenem Freibauer, Strafe wenn weit weg. Phase-gated, skaliert mit `(threshold - phase) / threshold`. |
| `rook_passed_pawn_bonus` / Passbauer-Rang-Bonus | `eval.rs:148`, `pawn_passed_rank_bonuses` | Bauer-Vorrücken belohnt, Turm-hinter-Bauer-Bonus. |

Die K-Aktivitäts-Achse ist also schon gut versorgt. **Was fehlt:**

1. Die **Opposition** zwischen den Königen — ein binäres, brettgeometrisches
   Konzept, das die bestehenden Terme nicht abdecken.
2. Die **Key Squares** vor einem eigenen Frei-/Bauer — die existierende
   K-zu-Bauern-Distanz ist linear, nicht „diese 3 Felder sichern den Gewinn".
3. Der **Rook-Pawn-Edge-Case** — bei a- oder h-Bauer + schwacher K in der
   Promo-Ecke ist die Stellung Remis, unabhängig davon, wie weit der Bauer
   gerückt ist. Der Passbauer-Bonus überschätzt diese Stellung aktuell.

## Drei Sub-Konzepte

### 1. Opposition

**Klassisches Konzept:** Zwei Könige in gerader Linie (Datei, Reihe oder
Diagonale) mit genau einem leeren Feld dazwischen. Wer **nicht** am Zug ist,
„hat die Opposition" und zwingt den anderen, ein Feld vor sich freizugeben.

**Generalisierte Form (Distanzopposition):** gleiche Linie, ungerade Anzahl
freier Felder (1, 3, 5), Seite am Zug verliert die Opposition.

**Eval-Term-Idee:**
```
opposition(us) = wenn (gleiche Linie ∨ gleiche Reihe ∨ gleiche Diagonale)
                 UND (Anzahl Felder zwischen K_us und K_them ist ungerade)
                 UND (board.side_to_move != us)
                THEN +opposition_bonus
                ELSE 0
```

Wert klein halten — Größenordnung 10–25 cp, weil Opposition oft nur
*indirekt* zum Vorteil führt (sie öffnet die richtige Folgestellung).
Wer am Zug ist und die Opposition halten **muss**, verliert sie typisch
in einem Halbzug — der Bonus wackelt zwischen den Halbzügen, das ist
erwünscht (Quiescence muss damit umgehen, aber der Term ist nur in
ruhigen Stellungen aktiv, also kein Quiescence-Pfad-Problem).

**Spezialfall:** wenn alle Bauern auf der gleichen Brettseite sind und
ein Frei-Bauer existiert, ist die Opposition **vor dem Bauer** wertvoller
als woanders — kann später als Verfeinerung kommen, nicht im ersten Wurf.

### 2. Key Squares vor eigenen Freibauern

**Klassisches Konzept:** Für jeden Bauern gibt es 3 Schlüsselfelder vor ihm,
deren Belegung durch unseren König den Gewinn (bzw. das Promo-Durchsetzen)
sichert.

- Bauer auf Rang 2/3/4 (weiß) / 7/6/5 (schwarz): Schlüsselfelder sind die
  3 Felder zwei Reihen vor dem Bauer (Bauer e2 → Schlüsselfelder d4/e4/f4).
- Bauer auf Rang 5 (weiß) / 4 (schwarz): zwei *Triplets* — direkt vor dem
  Bauer und zwei Reihen vor dem Bauer.
- Bauer auf Rang 6/7 (weiß) / 3/2 (schwarz): die 3 Felder direkt vor dem
  Bauer.

**Eval-Term-Idee:**
```
key_square_bonus(us) = Σ über alle eigenen Freibauern:
   wenn K_us auf einem der 3 Schlüsselfelder vor diesem Bauer
   THEN +key_square_bonus_by_rank[rank]
```

`key_square_bonus_by_rank` als kleine Tabelle in `EvalParams`, mit
ansteigenden Werten (Bauer auf Rang 5 hat höhere Belohnung als Rang 2).
Größenordnung 15–40 cp, weil das ein **starker Gewinn-Indikator** ist,
aber nur in Endspielen ohne ablenkende Officers.

**Lookup-Tabelle:** Pro Bauer-Square kann eine `[Square; 3]` vor dem Bauer
einmalig vorberechnet werden (`lazy_static!` oder `const fn`). 64 Bauern-
felder × 2 Farben = 128 Einträge à 3 Squares. Vernachlässigbar.

**Rook-Pawn-Ausnahme:** Bei a- und h-Bauern existiert das Konzept so nicht
(weil die Promo-Ecke vom verteidigenden K immer gehalten werden kann) —
Schlüsselfelder werden für diese Bauern nicht vergeben, stattdessen greift
Sub-Konzept 3.

### 3. Rook-Pawn-Edge

**Klassisches Konzept:** K + a-Bauer (oder h-Bauer) vs. K ist Remis, wenn
der schwache König die Promo-Ecke erreicht oder rechtzeitig dorthin kann.

**Eval-Term-Idee:**
```
rook_pawn_penalty(us) = wenn wir nur einen Frei-Bauer haben
                        UND der ist auf a- oder h-Linie
                        UND chebyshev(K_them, promo_corner) ≤ 1
                        UND |Σ Officer-Material| ≤ schwelle
                       THEN -rook_pawn_drawish_penalty * pawn_rank_bonus
                       ELSE 0
```

Wirkt als **Korrektur** auf den bestehenden Passbauer-Bonus — wir wollen
den Passbauer-Bonus nicht ersatzlos streichen (es gibt Stellungen, in
denen wir den a/h-Bauer trotzdem durchsetzen, wenn der Gegner taktisch
gebunden ist), sondern abschwächen.

## Aktivierungsbedingung

Damit der Term nicht in mittelgroße Endspiele hineinleckt:

```
gate:
  Σ NPM_white  ≤ npm_endgame_gate   (z.B. 700 cp = höchstens 2 leichte Figuren)
  Σ NPM_black  ≤ npm_endgame_gate
  Phase        <  king_activity_phase_threshold
```

Das `npm_endgame_gate` (~700 cp) muss noch festgelegt werden — Vorschlag
für den ersten Wurf, A/B-baren. Phase-Threshold ist bereits aus
`king_activity_endgame` vorhanden.

Zusätzliche Skalierung wie bei `king_passed_pawn_synergy`:
```
let eg_weight = (threshold - phase) / threshold;
final_score = raw_score * eg_weight;
```

So vermeiden wir Sprünge an der Phase-Grenze.

## Architektur im Code

Neue Funktion in `eval.rs`, additiv:

```rust
// in evaluate() neben king_act / king_pass_syn
let pawn_eg_guard = pawn_endgame_guard(board, phase, p);

non_pst + taper(mg, eg, phase) + king_act + king_pass_syn + mob + trap + pawn_eg_guard
```

Implementierungs-Skelett:

```rust
fn pawn_endgame_guard(board: &Board, phase: i32, p: &EvalParams) -> i32 {
    if !is_simple_endgame(board, p) { return 0; }
    if phase >= p.king_activity_phase_threshold { return 0; }

    let w = side_pawn_endgame_guard(board, Color::White, p);
    let b = side_pawn_endgame_guard(board, Color::Black, p);

    let eg_weight = p.king_activity_phase_threshold - phase;
    (w - b) * eg_weight / p.king_activity_phase_threshold
}

fn side_pawn_endgame_guard(board: &Board, us: Color, p: &EvalParams) -> i32 {
    let mut score = 0;
    score += opposition_bonus(board, us, p);
    score += key_square_bonus(board, us, p);
    score += rook_pawn_correction(board, us, p);
    score
}
```

Neue Parameter in `EvalParams` / `eval.toml`-Sektion `[endgame_guard]`:
- `opposition_bonus` (default ~15 cp)
- `key_square_bonus_by_rank: [i32; 8]` (z.B. `[0, 0, 10, 15, 25, 35, 30, 0]`)
- `rook_pawn_drawish_penalty` (z.B. 60 cp Reduktion)
- `npm_endgame_gate` (z.B. 700 cp)

Defaults aus Code-Sicht: **alle 0** außer `npm_endgame_gate` — damit das
Verhalten ohne TOML-Override identisch zum heutigen Stand ist (Tobias-
Pattern aus Dynmat-Step 1/2/3, [[project-dynmat-step1]]).

## Interaktion mit existierenden Termen

- **`king_passed_pawn_synergy`**: läuft additiv. Wenn K nahe am eigenen
  Passbauer ist UND auf einem Key Square steht, gibt es beide Boni —
  gewollt, weil Key Square > „nur in der Nähe".
- **`endgame::kpk_score`**: hat Vorrang. Wenn die Signatur strict KPK
  ist und der Bauer unaufhaltbar, liefert `endgame_score` einen
  abgeschlossenen Wert und der Eval-Pfad mit unserem Term wird gar nicht
  erreicht (`evaluate()` Z. 19).
- **`pawn_passed_rank_bonuses`**: davon profitiert auch der Rook-Pawn,
  was sub-konzept 3 explizit korrigiert. Doppelzählung an dieser Stelle
  ist die motivierende Lücke, kein Bug.
- **`king_exposure_penalty`**: NPM-Gate ≥1500 → in unserem Aktivierungs-
  bereich (NPM≤700 pro Seite) bereits inaktiv. Kein Konflikt.

## Risiken und Symptome

| Risiko | Symptom in der Auswertung |
|---|---|
| `npm_endgame_gate` zu hoch → Term lebt im Mittelspiel | `positional_collapse` steigt, mate-Werte stabil |
| `opposition_bonus` zu groß | Engine vermeidet K-Aktivität, weil Opposition halten lohnender wirkt als vorrücken → Endspiel-Shuffling steigt |
| Key-Square-Bonus überdreht | Engine schiebt K vor den Bauer, blockiert den eigenen Bauer (`positional_collapse`) |
| Rook-Pawn-Korrektur zu aggressiv | Engine gibt a/h-Bauer kampflos auf, weil sie ihn als wertlos einschätzt → `hangs_pawn`-artige Patzer |
| Phase-Übergang ungleichmäßig | Score-Sprünge an Phase-Grenze, Iteratives Deepening wackelt |

## Verifikations-Plan

**Stufe 1 — lokale Test-Stellungen:**
- `MH3BeAfV` ply 91 (`3k4/5p2/1N1P4/p1PK2p1/1n4P1/5P1p/7P/8 w - - 1 46`):
  vor dem Term Kd4-Kc4-Verschlimmbesserung, nach dem Term sollte die
  Eval-Differenz das richtige K-Manöver favorisieren.
- `HxAG2UV8` ply 129 (`8/6p1/1Pk1n2p/p3P2P/8/1p2K3/1B6/8 w - - 4 65`):
  Test, ob Engine die Opposition zwischen Ke3 und Kc6 hält.
- Klassische KPK-Lehrbuchstellungen aus Dvoretsky/Müller (Opposition vs
  Schlüsselfelder vs Rook-Pawn) — Erwartung: Eval kippt in die richtige
  Richtung.
- Eine `npm_endgame_gate`-Grenze-Stellung (z.B. KRP vs KRP): Term darf
  hier **nicht** aktiv werden.

**Stufe 2 — Selbst-vs-Selbst:**
- 1000 Partien fastchess SPRT[0, 10] gegen Baseline ohne Term, 5+0.05 UHO
  (Standard-Setup [[reference-match-runner]]).
- Erfolgs-Kriterium: positive Elo-Differenz, oder zumindest nicht
  signifikant negativ + saubere Lichess-Lookback-Daten.
- Selfplay-A/B-Caveat gilt: [[feedback-ab-vs-lichess-signal]] — bei
  „CI deckt Null"-Ergebnis nicht automatisch verwerfen, sondern Lichess-
  Lookback abwarten.

**Stufe 3 — Lichess:**
- Rollout, ≥150 Partien sammeln.
- Auswertung mit `analyze_blunders.py --report` gegen den 23.05.-Stand.
- Ziel-KPIs:
  - stickshark99 B/P: 2.16 → ~1.5
  - allgemeines `positional_collapse` im Endspiel: stabil oder sinkend
  - kein Anstieg bei `hangs_pawn`/`exposed_king`
  - Rating: +5 bis +20 Elo (kleiner Effekt, weil Term nur ein Sub-Set
    der Stellungen erreicht).

## Eigenleistung — Aufgabenteilung

Was Tobias selbst macht:
- Festlegen der konkreten Werte für `opposition_bonus`,
  `key_square_bonus_by_rank`, `rook_pawn_drawish_penalty`,
  `npm_endgame_gate`.
- Schreiben der drei Hauptfunktionen (`opposition_bonus`,
  `key_square_bonus`, `rook_pawn_correction`) — die Schach-Logik gehört
  zu seiner Eigenleistung [[feedback-eigenleistung]].
- Entscheidung über die Aktivierungs-Schwelle und Reihenfolge der drei
  Sub-Konzepte (z.B. erst nur Opposition implementieren, dann
  Key Squares, dann Rook-Pawn — analog zum Dynmat-Step-Pattern).
- Bewertung der Mess-Ergebnisse und Anpassungen.

Was Claude liefern darf (Infrastruktur, auf Anfrage):
- Lookup-Tabelle der Key Squares pro Bauern-Square × Farbe.
- Helper-Funktion „Anzahl freier Felder zwischen zwei Squares auf einer
  Linie/Reihe/Diagonale" — reine Brettgeometrie.
- Erweiterung des `EvalParams`-Structs um die neuen Felder, inkl.
  TOML-Loader.
- Test-Skript zur Verifikations-Stufe 1 (Engine-Eval auf Lehrbuch-FENs).

Engine-Logik bleibt Tobias' Entscheidung; Claude zeigt Optionen, Tobias
wählt.

## Klärungen vor der Implementierung

Vor dem Code:

1. **Reihenfolge der Sub-Konzepte:** alle drei in einem Wurf, oder
   Step-Pattern (erst Opposition, A/B, dann Key Squares, A/B, dann
   Rook-Pawn)?
2. **`npm_endgame_gate` als hartes Gate oder weiches Tapering?** Hart
   ist einfacher und besser testbar; weich (Übergang zwischen 700 cp
   und 1400 cp) vermeidet Sprünge, ist aber komplexer.
3. **Opposition-Definition:** nur direkte Opposition (1 Feld dazwischen)
   im ersten Wurf, oder auch Distanzopposition (3/5 Felder)?
   Empfehlung: direkt + diagonal im ersten Wurf, Distanz später.
4. **Key-Square-Tabelle für Doppelbauern:** unklar, ob bei zwei eigenen
   Bauern auf derselben Linie beide Bauern Schlüsselfelder beanspruchen
   oder nur der hintere. Empfehlung: nur der weiter vorgerückte —
   verhindert Doppelzählung.
5. **Wird der Term auch in `evaluate_breakdown()` ausgewiesen?** Ja —
   analog zu allen anderen Termen, damit `eval`-UCI-Kommando den
   Beitrag pro Stellung zeigt (notwendig fürs Debuggen, siehe
   `validate_connected_rooks_*.txt`-Workflow vom 15.05.).

## Nächster Schritt

Wenn das Konzept abgenommen ist:
1. Stufe-1-Stellungen ausarbeiten (~5–8 FENs aus Lehrbuch + 3 aus den
   stickshark99-Cluster).
2. Tobias entscheidet zu Klärungen 1–4.
3. Implementierung in der gewählten Reihenfolge.
4. A/B-Match nach Stufe 2.
