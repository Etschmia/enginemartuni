# Static Exchange Evaluation (SEE)

Dieses Dokument erklärt, **was SEE ist**, warum Martuni es braucht, und wie wir
es implementieren. Es ist als Lern-Referenz für Tobias geschrieben — kein
Fachchinesisch, sondern Schritt für Schritt.

## Was ist das Problem?

Martuni spielt Züge wie 22...Nh5?? (Springer wird von der Dame geschlagen) oder
18...Bxd2?? (Läufer wird schlecht getauscht). Die Engine "sieht" den Rückschlag
nicht rechtzeitig, weil die Suchtiefe nicht ausreicht.

**Warum reicht die Tiefe nicht?**
Weil die Quiescence-Suche gerade ALLE Schlagzüge durchsucht — auch solche, die
offensichtlich Material verlieren (Dame schlägt gedeckten Bauern, wird
zurückgeschlagen). Das kostet so viel Rechenzeit, dass die effektive Suchtiefe
sinkt und selbst einzügige Einsteller übersehen werden.

## Was macht SEE?

SEE beantwortet eine einfache Frage:

> **"Wenn auf Feld X eine Schlagserie stattfindet — wer schlägt wen, mit welcher
> Figur, in welcher Reihenfolge — wie viel Material gewinnt oder verliert die
> Seite, die den ersten Schlag macht?"**

### Beispiel: Läufer schlägt gedeckten Springer

Stellung: Weißer Läufer (330cp) schlägt schwarzen Springer (320cp) auf e5.
Schwarzer Bauer auf d6 deckt e5.

```
Schritt 1: Bxe5       → Weiß gewinnt 320 (Springer)
Schritt 2: dxe5       → Schwarz gewinnt 330 (Läufer) zurück
Saldo: 320 - 330 = -10 cp für Weiß
```

SEE sagt: **-10 cp** — der Tausch ist minimal schlecht für Weiß.

### Beispiel: Dame schlägt gedeckten Bauern

Stellung: Weiße Dame (900cp) schlägt schwarzen Bauern (100cp) auf d5.
Schwarzer Springer auf c3 deckt d5.

```
Schritt 1: Qxd5       → Weiß gewinnt 100 (Bauer)
Schritt 2: Nxd5       → Schwarz gewinnt 900 (Dame) zurück
Saldo: 100 - 900 = -800 cp für Weiß
```

SEE sagt: **-800 cp** — katastrophal. Diesen Schlagzug sollte die Quiescence
**sofort abschneiden** statt ihn voll zu durchsuchen.

### Beispiel: Springer schlägt ungedeckten Läufer

```
Schritt 1: Nxe5       → Weiß gewinnt 330 (Läufer)
Schritt 2: —           → Schwarz hat keinen Angreifer mehr
Saldo: +330 cp für Weiß
```

SEE sagt: **+330 cp** — klarer Gewinn, sofort durchsuchen.

### Die Schlüsselidee: jede Seite darf aufhören

In einer echten Schlagserie muss **niemand zurückschlagen**. Wenn Schwarz nach
Bxe5 sieht, dass dxe5 sofort von Rxe5 beantwortet wird, kann Schwarz einfach
*nicht* zurückschlagen und den Läufer-Verlust akzeptieren statt auch noch den
Bauern zu verlieren.

SEE simuliert das mit einem **Minimax auf dem einzelnen Feld**: nach jedem
Schlag prüft die Seite am Zug, ob Weiterschlagen besser ist als Aufhören.

## Wie funktioniert SEE algorithmisch?

```
see(board, move) -> i32:
    1. Führe den Schlagzug aus. Gewinn = Wert der geschlagenen Figur.
    2. Finde den billigsten Angreifer der Gegenseite auf das Zielfeld.
    3. Wenn kein Angreifer: fertig, return Gewinn.
    4. Simuliere den Rückschlag (entferne Angreifer, Gewinn -= Wert der
       gerade geschlagenen Figur).
    5. Wiederhole ab Schritt 2 mit der anderen Seite.
    6. Am Ende: Minimax rückwärts — jede Seite nimmt das Maximum aus
       "aufhören" und "weiterschlagen".
```

### Pseudocode

```
fn see(board, capture_move) -> i32:
    target_square = capture_move.destination
    
    // Gain-Array: was gewinnt die jeweilige Seite in jedem Schritt
    gains = []
    gains[0] = wert_der_figur_auf(target_square)
    
    // Bitboard aller Angreifer auf target_square (beide Seiten)
    attackers = alle_angreifer(board, target_square)
    
    // Die Figur die gerade geschlagen hat steht jetzt auf target_square
    current_piece = figur_die_zieht(capture_move)
    side = gegenseite(capture_move)
    
    depth = 0
    loop:
        depth += 1
        gains[depth] = wert(current_piece) - gains[depth - 1]
        
        // Billigsten Angreifer von `side` finden
        attacker = billigster_angreifer(attackers, side)
        if kein attacker: break
        
        // Angreifer vom Bitboard entfernen (aufdeckt evtl. Gleiter dahinter)
        attackers.entferne(attacker)
        attackers |= aufgedeckte_gleiter(board, attacker, target_square)
        
        current_piece = figur_auf(attacker)
        side = !side
    
    // Minimax rückwärts: jede Seite nimmt max(aufhören, weiterschlagen)
    while depth > 0:
        gains[depth - 1] = -max(-gains[depth - 1], gains[depth])
        depth -= 1
    
    return gains[0]
```

### Was bedeutet "aufgedeckte Gleiter"?

Wenn ein Springer von c3 nach e5 schlägt, könnte dahinter ein Läufer auf a1
stehen, der jetzt e5 angreift (die Diagonale ist frei). SEE muss diese
**X-Ray-Angriffe** berücksichtigen:
- Türme und Damen durch Reihen und Spalten
- Läufer und Damen durch Diagonalen

Deshalb entfernen wir den Angreifer vom Bitboard und schauen, ob dadurch neue
Angreifer auf das Feld sichtbar werden.

## Wo setzen wir SEE ein?

### 1. Bad Capture Pruning in Quiescence (Hauptzweck)

Aktuell (`search.rs`, Zeile 437-457) durchsucht die Quiescence **alle**
Schlagzüge. Mit SEE:

```rust
for mv in captures {
    if see(board, mv) < 0 {
        continue;  // Verlierende Schlagzüge überspringen
    }
    // ... rest wie bisher
}
```

**Erwartete Wirkung:** Drastisch weniger Knoten in taktischen Stellungen →
höhere effektive Suchtiefe → einzügige Einsteller werden sichtbar.

### 2. Move Ordering (Zukunft, aktuell zu teuer)

Idee: SEE-Wert als Sortierkriterium, verlierende Captures hinter Quiet Moves.
**Ergebnis beim Test:** SEE in `order_moves` verdoppelt die Knotenanzahl, weil
SEE pro Knoten ~30 Mal aufgerufen wird (einmal pro legalem Capture). MVV/LVA
bleibt vorerst besser — die Sortierung ist "gut genug" und kostenlos.

**Nächster Schritt (wenn SEE optimiert ist):** SEE-Wert cachen pro Zug, dann
in Ordering nutzen. Oder: nur für die ersten N Captures SEE berechnen.

### 3. Selektive Extensions (Zukunft, aktuell zu teuer)

Idee: nur Captures mit `see(mv) >= 0` extenden.
**Ergebnis beim Test:** Gleiche Performance-Kosten wie bei Move Ordering.
Bleibt offen für nach der SEE-Optimierung.

## Implementierungsentscheidungen

### Wo lebt der Code?

Neue Funktion `see()` in `search.rs` (neben `mvv_lva_key` und `is_capture`).
Kein eigenes Modul nötig — SEE ist eine reine Such-Hilfsfunktion.

### Figurenwerte für SEE

Wir nehmen die Material-Werte aus `eval_config.rs` (P=100, N=300, B=300,
R=500, Q=900, K=100000). Der Königswert ist absurd hoch, weil ein "König
schlägt" nur passiert wenn es die letzte Figur in der Kette ist — der König
kann nicht geschlagen werden, aber er kann selbst schlagen.

### Bitboard-Operationen

Das `chess`-Crate liefert uns:
- `board.pieces(Piece::X)` → Bitboard aller Figuren eines Typs
- `board.color_combined(Color::X)` → Bitboard aller Figuren einer Farbe
- Angreifer-Lookup über Rays/Attacks

Wir brauchen: eine Funktion, die alle Angreifer auf ein Feld findet, inkl.
X-Ray-Angriffe nach Entfernung einer Figur.

### Performance

SEE wird **sehr häufig** aufgerufen (für jeden Schlagzug in Quiescence und
Move Ordering). Deshalb:
- Nur Bitboard-Operationen, keine Board-Copies
- Kein `make_move` — alles wird auf Bitboard-Ebene simuliert
- Gain-Array auf dem Stack (max. 32 Einträge, in der Praxis < 10)

## Umsetzungsstatus (2026-04-12 / Update 2026-04-14)

### Erledigt

1. **`see()` implementiert** in `search.rs`
   - `see_piece_value()` — Materialwerte für SEE
   - `all_attackers_to()` — alle Angreifer auf ein Feld (inkl. X-Ray)
   - `least_valuable_attacker()` — billigsten Angreifer finden
   - `see()` — Hauptfunktion mit Gain-Array und Minimax-Rückfaltung
2. **Quiescence: Bad Capture Pruning** — `see(mv) < 0` → skip

### Messergebnis (Stella-B-Position, 5s)

| Version    | Tiefe | Knoten | Zeit   | Zug  |
|------------|-------|--------|--------|------|
| Ohne SEE   | 2     | 3.4M   | 1826ms | Re8  |
| Mit SEE    | 2     | 2.5M   | 1422ms | Rd1  |
| Verbesserung | —  | **-27%** | **-22%** | —  |

3. **Selektive Extensions mit SEE** — implementiert (2026-04-13): Captures in
   `is_candidate_move` nur noch extenden wenn `see(mv) >= 0`.

---

## Kritischer Bug-Fix: SEE-Logik invertiert (2026-04-13)

### Was war falsch?

In `see()` wurde `gain[depth]` **vor** der Prüfung berechnet, ob überhaupt ein
Angreifer existiert:

```rust
// BUGGY (alt):
loop {
    depth += 1;
    gain[depth] = current_value - gain[depth - 1];  // ← Phantom-Eintrag
    let Some(attacker) = least_valuable_attacker(...) else { break };
    ...
}
```

Das erzeugte am Ende der Schlagserie immer einen fiktiven "letzten Zug", der
die Minimax-Rückfaltung komplett umkehrte:

| Szenario | SEE buggy | SEE korrekt |
|---|---|---|
| Bxungedeckter_Bauer | **-200** | **+100** |
| Bxa7 + Rxa7 | **+100** | **-200** |

**Gewinnende Captures bekamen negative SEE → wurden in Quiescence gepruned.**  
**Verlierende Captures bekamen positive SEE → blieben ungepruned.**

### Auswirkung

Die Quiescence-Suche sah **keine** eigenen Gewinnzüge (gepruned) und ließ
**Materialopfer des Gegners** unbewertet (nicht gepruned). Ergebnis: die Engine
spielte wiederholt sinnlose Figuren-Opfer, weil der Rückschlag des Gegners in
der Quiescence verschwand.

Konkret Partie `rds8gwiN`, Zug 14. Bxa7:
- Engine dachte: +142cp (Läufer auf a7, Quiescence sieht Rxa7 nicht)
- Tatsächlich: -200cp Material + Rückschlag Rxa7

### Fix

```rust
// KORREKT (neu):
loop {
    let Some(attacker) = least_valuable_attacker(...) else { break }; // erst prüfen
    depth += 1;
    gain[depth] = current_value - gain[depth - 1];  // dann berechnen
    ...
}
```

### Folgeänderung: Quiescence-Begrenzung

Durch den Fix werden korrekt mehr Captures in der Quiescence erkundet
(vorher: alle fälschlich gepruned). Ohne Begrenzung würde die Quiescence
exponentiell wachsen. Hinzugefügt:

- **`MAX_QPLY = 12`** — Tiefenlimit ab dem Stand-Pat zurückgegeben wird
- **Delta Pruning** — Capture, der selbst mit `see_val + DELTA_MARGIN = 200cp`
  alpha nicht erreichen kann, wird übersprungen

---

## Regression-Analyse (2026-04-14)

173 Blunder aus ~40 Partien nach dem SEE-Fix analysiert.

### Vergleich alt (103 Bl., 14 Partien) → neu (173 Bl., ~40 Partien)

| Motiv | Alt % | Neu % | Trend |
|---|---|---|---|
| unclassified | 39% | 38% | = |
| allows_mate | 25% | 19% | ✓ SEE wirkt |
| king_safety | 13% | 13% | = |
| hangs_bishop | 13% | 9% | ✓ |
| hangs_knight | 9% | 6% | ✓ |
| missed_capture | 8% | 11% | ✗ gestiegen |
| positional_collapse | 6% | 10% | ✗ gestiegen |
| hangs_rook | 3% | 6% | ✗ gestiegen |

**Fazit:** SEE hat taktische Fehler messbar reduziert. Neue Prioritäten:

1. `missed_capture` ↑ → Delta-Margin war zu aggressiv
2. `endgame king_safety` (13 von 39 Endspiel-Blundern) → König ohne Aktivitäts-Eval
3. `positional_collapse` + `unclassified` → Rook-Aktivität fehlt in der Eval
4. Analyzer-Noise: ~10 Einträge wo `gespielter Zug == SF-Best` (false positives)

### Analyzer-Bug: false positives

Mehrere `allows_mate`-Einträge hatten `best=gespielter Zug`:
```
29. g3  loss=99242cp  best=g3  ← kein Blunder, Martuni spielte optimal
49. c4  loss=98789cp  best=c4  ← gleiche Situation
52. Kc2 loss=610cp    best=Kc2 ← gleiche Situation
```
**Ursache:** Analyzer zählte "Position war nach bestem Zug trotzdem verloren" als Blunder.
**Fix (`analyze_blunders.py`, 2026-04-14):** Einträge mit `move == best_move` werden
jetzt übersprungen.

---

## Implementierungsschritte (2026-04-14)

### D) Analyzer-Bug fixen (false positives)

**Datei:** `tools/analyze_blunders.py`

Neue Bedingung in `analyze_game()` direkt nach `if loss >= threshold_cp:`:
```python
# Kein echter Blunder: Martuni spielte exakt den SF-empfohlenen Zug.
if best_move is not None and move == best_move:
    board.push(move)
    continue
```

**Erwartete Wirkung:** ~5–10% weniger gemeldete Blunder, sauberere Daten.

### B) Rook auf offenen / halb-offenen Linien

**Datei:** `src/eval.rs` (neue Funktion `rook_file_bonus`), `src/eval_config.rs`, `eval.toml`

Neue Parameter:
- `rook_open_file_bonus = 30` — keine Bauern beider Seiten auf der Linie
- `rook_semiopen_file_bonus = 15` — keine eigenen, aber gegnerische Bauern

Aufruf in `evaluate_side()` für jeden Turm.

**Erwartete Wirkung:** Turm-Aktivierung wird belohnt → weniger `positional_collapse`
und `unclassified` wo Tuerme passiv stehen.

### A) Endspiel-König-Aktivität

**Datei:** `src/eval.rs` (neue Funktionen `king_activity_endgame`,
`king_centralization_score`), `src/eval_config.rs`, `eval.toml`

Neue Parameter:
- `king_activity_bonus = 3` — cp pro Zentralisierungseinheit
- `king_activity_phase_threshold = 16` — wirkt bei Phase < 16 (≈ 2/3 Material weg)

Formel: `(w_score - b_score) * (threshold - phase) * bonus / threshold`
Zentralisierungsscore: 7 = Zentrum (d4-e5), 0 = Ecke. Max-Bonus: ~21 cp.

Aufruf in `evaluate()` nach dem Tapered PST.

**Erwartete Wirkung:** König sucht im Endspiel aktiver das Zentrum → weniger
`endgame king_safety`-Blunder.

### C) Delta-Pruning-Margin 200 → 150

**Datei:** `src/search.rs`, Konstante `DELTA_MARGIN`

**Begründung:** `missed_capture` stieg von 8% auf 11%. `DELTA_MARGIN = 200` prunte
gute Captures (stand_pat + see_val + 200 < alpha), wenn die Position bereits leicht
negativ war. Mit 150 werden mehr Gewinnzüge bewertet.

**Trade-off:** Leicht mehr Quiescence-Knoten in hoffnungslosen Stellungen, dafür
weniger verpasste Gewinnzüge.

---

---

## Regression-Analyse (2026-04-15)

225 Blunder aus ~60 Lichess-Partien nach König-Aktivität + Rook-Boni + Delta-Margin 150 analysiert.
Lichess-Wertung in dieser Phase: Blitz 1530 → 1659, Rapid 1680 → 1756.

### Vergleich 14.04 (173 Bl.) → 15.04 (225 Bl.)

| Motif | 14.04 | 15.04 | Trend |
|---|---|---|---|
| unclassified | 38% | 36% | = |
| **missed_capture** | 11% | **18%** | ✗ weiter gestiegen |
| allows_mate | 19% | 14% | ✓ |
| **hangs_bishop** | 9% | **14%** | ✗ gestiegen |
| king_safety | 13% | 8% | ✓ König-Aktivität wirkt |
| positional_collapse | 10% | 9% | ≈ |
| hangs_knight | 6% | 6% | = |

**Gewinne:** `king_safety` und `allows_mate` deutlich gesunken — König-Aktivitäts-Bonus
+ Rook-Boni zeigen Wirkung. **Offene Baustellen:**

1. `missed_capture` ↑ — Delta-Margin allein reicht nicht; MVV/LVA-Sortierung findet
   taktische Captures nicht zuverlässig. Lösung: SEE-basiertes Move Ordering (Punkt 4).
2. `hangs_bishop` ↑ — mittelbare Opfer aus Selbstüberschätzung, die nur durch
   tiefere Suche (→ besseres Ordering) sichtbar werden.
3. Endgame-König-Überzieher: `Kd4/Kf4/Kf3 allows_mate` in mehreren Endspielen.
   König-Aktivitäts-Bonus zieht zu forsch nach vorn wenn Schwerfiguren noch da sind.

---

## Implementierungsschritte (2026-04-15)

### 1) Move Ordering mit SEE + Cache

**Datei:** `src/search.rs`

Ziel: SEE pro Capture genau einmal berechnen, Ergebnis durch Ordering +
Extension-Check + Quiescence teilen; gewinnende Captures landen konsistent vor
Quiet Moves, verlierende Captures landen dahinter.

**Änderungen:**
- Neue Struktur `ScoredMove { mv, order_key, see_val: Option<i32> }` ersetzt
  `Vec<ChessMove>` als Rückgabewert von `order_moves`.
- Sortier-Hierarchie:
  | Prio | Bedingung | `order_key` |
  |---|---|---|
  | 1 | TT-Move | -100_000 |
  | 2 | Promotion zu Dame | -50_000 |
  | 3 | Capture mit SEE ≥ 0 | -40_000 + MVV/LVA |
  | 4 | Andere Promotion | -500 |
  | 5 | Quiet Move | 0 |
  | 6 | Capture mit SEE < 0 | 10_000 − SEE |
- `is_candidate_move` bekommt `see_val: Option<i32>` als Parameter — kein zweiter
  `see()`-Aufruf mehr in der Extension-Entscheidung.
- Quiescence sortiert Captures jetzt nach SEE absteigend statt MVV/LVA → frühere
  Beta-Cutoffs. SEE wird pro Capture genau einmal berechnet und direkt
  weiterverwendet.

**Erwartete Wirkung:**
- `missed_capture` sinkt, weil gewinnende Captures in der Hauptsuche konsistent
  vor Quiet Moves untersucht werden.
- Mehr Alpha-Beta-Cutoffs durch verlierende Captures am Ende.
- Keine Netto-Mehrkosten für SEE: 1× je Capture statt bis zu 2× (Quiescence +
  Extension-Check).

**Smoke-Test (Kiwipete, 3s):** Tiefe 2, 4.0M Knoten, 1.6M NPS, Bestzug e2a6 — sauber.

### 2) Endgame-König-Guard

**Datei:** `src/eval.rs` (neue Funktion `heavy_piece_threat`)

Ziel: Verhindert, dass der König-Aktivitäts-Bonus den König zu früh ins Zentrum
zieht, solange der Gegner noch gefährliche Schwerfiguren hat. Motiviert durch
`Kd4/Kf4/Kf3 allows_mate`-Einträge in mehreren Endspielen der 15.04-Analyse.

**Änderung:** In `king_activity_endgame` wird der Zentralisierungs-Score pro
Seite unterdrückt (auf 0 gesetzt), wenn der Gegner eine Dame oder mehr als
einen Turm hat. KRvK und KQvK bleiben unberührt.

```rust
fn heavy_piece_threat(board, side) -> bool {
    queens(side) > 0 || rooks(side) > 1
}
```

**Test-Update:** `rooks_not_connected_when_blocked` — der schwarze König
verliert seinen Aktivitätsanteil (Weiß hat 2 Türme), Weiß behält seinen
(Schwarz hat nichts). Erwartungswert 1401 → 1409.

**Erwartete Wirkung:** Weniger Endspiel-König-Überzieher. Unauffällig in
ausgeglichenen Endspielen, wo beide Seiten symmetrische Schwerfiguren haben
und sich der Effekt aufhebt.

---

## Regression-Analyse (2026-04-16)

113 Blunder aus 53 Partien seit dem 15.04-Commit ausgewertet. Spielstärke-Trend
war minimal (im Rahmen der üblichen Schwankung), die Motiv-Verschiebungen sind
aber aussagekräftig.

### Vergleich 15.04 (225 Bl., ~60 Partien) → 16.04 (113 Bl., 53 Partien)

| Motiv | 15.04 | 16.04 | Trend |
|---|---|---|---|
| unclassified | 36% | 32% | ≈ |
| **missed_capture** | **18%** | **17%** | ≈ nur marginal |
| allows_mate | 14% | **19%** | ✗ gestiegen |
| **hangs_bishop** | **14%** | **7%** | ✓ halbiert |
| positional_collapse | 9% | **16%** | ✗ stark gestiegen |
| king_safety | 8% | 5% | ✓ |
| hangs_knight | 6% | 4% | ✓ |

### Einschätzung

- **`hangs_bishop` halbiert** — sehr wahrscheinlich der SEE-Ordering-Effekt:
  gewinnende Captures konsistent vor Quiet Moves → mehr Beta-Cutoffs → tiefere
  effektive Suche → eigene Hänger werden sichtbar.
- **`missed_capture` nur marginal gesunken** — das war die ursprüngliche
  Zielmetrik von SEE-Ordering. Effekt zu klein. Ordering allein reicht nicht;
  Quiet-Moves brauchen ebenfalls eine bessere Sortierung (Killer/History),
  damit gegnerische Captures früh widerlegt werden.
- **`allows_mate` gestiegen** — vor allem Endspiel-Fälle mit zentralen
  König-Zügen: `Martuni vs WolfuhfuhBot` 40.Kc2/44.Ke3, `Martuni vs simbelmyne`
  41.Kc2, `Martuni vs bfiedler-bot` 29.f4. Der 15.04-König-Guard greift nur
  bei Dame oder 2 Türmen — ein einzelner Turm plus Leichtfigur reicht aber
  auch für Mattnetze gegen den zentralen König.
- **`positional_collapse` gestiegen** — meist späte Mittelspielfehler ohne
  klaren taktischen Patzer. Mobility fehlt in der Eval; bleibt vorerst offen.

### Nächste Iteration

Null-Move-Pruning ist zu früh: es hilft primär durch tiefere Suche, aber
Endspiel-allows_mate ist bereits das größte Problem, und NMP ist berüchtigt
für Zugzwang-Bugs im Endspiel. Ohne Killer/History fehlt außerdem die für
NMP typische scharfe Zugsortierung (NMP lebt von guten Widerlegungszügen).

Deshalb:

1. **King-Guard schärfen** — direkt gegen den allows_mate-Peak.
2. **Killer Moves / History-Heuristic** — gegen den missed_capture-Bodensatz
   und als Vorarbeit für späteres NMP.

---

## Implementierungsschritte (2026-04-16)

### 1) King-Guard: Turm + Leichtfigur aufnehmen

**Datei:** `src/eval.rs` (`heavy_piece_threat`)

Der Guard aus 15.04 triggerte nur bei **Dame oder 2 Türmen**. Die 16.04-Analyse
zeigt mehrere `allows_mate`-Fälle mit einzelnem Turm + Leichtfigur (z.B. bei
WolfuhfuhBot, simbelmyne, bfiedler-bot). Neue Regel:

```rust
fn heavy_piece_threat(board: &Board, side: Color) -> bool {
    // Dame
    if queens(side) > 0 { return true; }
    // 2+ Türme
    if rooks(side) >= 2 { return true; }
    // Turm + Leichtfigur
    if rooks(side) >= 1 && (bishops(side) + knights(side)) >= 1 { return true; }
    false
}
```

Unberührt bleiben reine **KRvK**, **KBvK**, **KNvK**, **KBBvK** — dort bleibt
der König-Aktivitäts-Bonus aktiv, damit die spezialisierten Endgame-Routinen
nicht durch den Guard gelähmt werden. Der Test `rooks_not_connected_when_blocked`
bleibt unverändert (Weiß hat 2 Türme → Schwarz weiterhin als bedroht markiert).

**Erwartete Wirkung:** Weniger `Kd4/Kc2/Ke3 allows_mate`-Einträge in
Endspielen mit einem Turm und begleitendem Leichtfigurenspiel.

### 2) Killer Moves + History-Heuristic

**Datei:** `src/search.rs`

SEE-Ordering allein drückt `missed_capture` kaum. Grund: sobald der
Suchbaum tief genug geht, findet sich eine Widerlegung meist erst nach
mehreren ruhigen Zügen. Quiet-Moves werden aber noch zufällig sortiert.
Killer/History lernen aus dem Suchbaum selbst, welche Quiet-Moves
typischerweise Beta-Cutoffs erzeugen.

**SearchState-Erweiterung:**
- `killers: [[Option<ChessMove>; 2]; MAX_PLY]` — 2 Slots je ply
  (MAX_PLY = 128 wegen Extensions über MAX_DEPTH hinaus).
- `move_history: Vec<i32>` (Länge 2×64×64) — indexiert als
  `[side][from*64 + to]`, auf `MAX_HISTORY = 16_000` geclampt.

**Update bei Beta-Cutoff** (`alpha_beta`, wenn `alpha >= beta`):
- Nur wenn der verursachende Zug ein Quiet-Move ist (kein Capture, keine
  Promotion): als Killer eintragen (Slot 0 ← Slot 1 Shift) und
  `history[side][from][to] += depth*depth`.

**Sortier-Hierarchie (`order_moves`):**

| Prio | Bedingung | `order_key` |
|---|---|---|
| 1 | TT-Move | -100_000 |
| 2 | Promotion zu Dame | -50_000 |
| 3 | Capture mit SEE ≥ 0 | -40_000 + MVV/LVA |
| 4 | Killer 1 | -30_000 |
| 5 | Killer 2 | -25_000 |
| 6 | Unterpromotion | -20_000 |
| 7 | Quiet Move | -history (Range [-16_000, 0]) |
| 8 | Capture mit SEE < 0 | 10_000 − SEE |

MAX_HISTORY wurde bewusst auf 16_000 geclampt: kleiner als der Abstand
Killer → Unterpromotion (5_000), damit die Prio-Reihenfolge Capture → Killer →
Unterpromotion → Quiet nicht kippt.

**Smoke-Test (Kiwipete, 3s):** Tiefe 2, 3.8M Knoten, 1.49M NPS, Bestzug
`e2a6` — identisch zum 15.04-Ergebnis, ~5% weniger Knoten auf gleicher Tiefe.

**Erwartete Wirkung:**
- `missed_capture` sollte messbar sinken: gegnerische Widerlegungen nach
  ruhigen Zügen werden früher gefunden.
- `unclassified` sollte mit sinken (Oberbegriff für schwache Zugsortierung).
- Keine direkte Wirkung auf `allows_mate` oder `positional_collapse` — dafür
  sind Guard-Verschärfung bzw. zukünftige Eval-Erweiterungen zuständig.

---

### Stichtag 16.04.2026 — zweites Messfenster

Martuni spielt jetzt einige Tage unbeaufsichtigt (Dienstag nächste Woche
liegen mehrere hundert Partien zur Auswertung vor). Keine weiteren
Änderungen, bis die neue Blunder-Analyse da ist.

---

## Regression-Analyse (2026-04-21)

322 Blunder aus **154 Partien** seit dem 16.04-Commit (`27c10ea
search+eval: Killer/History-Heuristic, König-Guard auf R+Minor verschärft`).
Rating-Entwicklung im gleichen Zeitraum:

| Zeitformat | 16.04 16:00 | 21.04 09:00 | Delta |
|---|---|---|---|
| Blitz    | 1662 | **1733** | **+71** |
| Rapid    | 1771 | **1842** | **+71** |

### Blunder/Partie

- 16.04: 113 / 53 ≈ **2.13 Bl./Partie**
- 20.04: 322 / 154 ≈ **2.09 Bl./Partie**

→ Die Rate pro Partie ist praktisch konstant, Rating aber +71 in beiden
Formaten. Der Gewinn stammt **nicht** aus weniger Blundern pro Partie,
sondern aus **besserer Qualität der übrigen Züge** — Killer/History +
SEE-Ordering + R+Minor-Guard heben das Baseline-Spiel sichtbar an, auch
wenn der Blunder-Floor noch gleich hoch ist.

### Motiv-Vergleich 16.04 → 20.04

| Motiv | 16.04 % | 20.04 % | Trend | Absolut 20.04 |
|---|---|---|---|---|
| unclassified | 32% | 38% | ✗ gestiegen | 124 |
| **allows_mate** | **19%** | **15%** | ✓ Guard wirkt | 48 |
| **missed_capture** | **17%** | **14%** | ✓ Killer/History wirkt | 45 |
| **king_safety** | **5%** | **12%** | ✗✗ mehr als verdoppelt | 39 |
| positional_collapse | 16% | 11% | ✓ | 34 |
| hangs_knight | 4% | 7% | ✗ | 24 |
| hangs_bishop | 7% | 6% | ≈ | 19 |
| hangs_rook | — | 5% | — | 15 |
| missed_mate | — | 4% | — | 14 |
| hangs_queen | — | 2% | — | 5 |

### Was gut funktioniert hat

- **R+Minor-Guard:** `allows_mate` sinkt von 19% auf 15% trotz dreifach
  größerem Sample. Die Endspiel-Mattnetze, die am 16.04 noch häufig waren
  (`Kd4/Kc2/Ke3`), sind seltener geworden.
- **Killer/History:** `missed_capture` sinkt von 17% auf 14%, und
  `positional_collapse` von 16% auf 11% — beides Motive, die von besserer
  Zugsortierung profitieren (gegnerische Widerlegungen werden früher
  gefunden, mehr Alpha-Beta-Cutoffs).
- **Rating +71 in beiden Formaten** in 4 Tagen ist klar signifikant und
  der stärkste bisher gemessene Sprung nach einem einzelnen Commit-Batch.

### Neue Sorgenkinder

1. **`king_safety` verdoppelt** (5% → 12%), 19 von 39 Fällen im Endspiel.
   Der R+Minor-Guard fängt gedeckte Schwerfiguren-Mattnetze ab, aber
   nicht **reine Pawn-Endgames** mit wanderndem König. Beispielcluster
   (CCI-7 in einer einzigen Partie `BGOX2GcM`): 73. Kf5 / 88. Kf6 / 92. f3 /
   152. Kd3 / 167. Kb3 — fünf `allows_mate` mit `martuni=+0cp` während
   SF +400–500cp Vorsprung sieht. Martuni kennt in KP-Endspielen weder
   Opposition noch Square-of-the-Pawn.

2. **`unclassified` gestiegen** (32% → 38%) — 97 davon im Mittelspiel.
   Klassisch Mobility-Defizit: passive Türme/Läufer werden belohnt solange
   sie irgendwo stehen. Kein einzelner Fix, aber eine Mobility-Metrik
   würde den ganzen Bodensatz anheben.

3. **`hangs_knight` leicht gestiegen** (4% → 7%). Meist sind das aber
   Randfälle (Springer wird durch Pin / Fork-Drohung durchschaut), nicht
   das frühere "Springer hängt trivial". Mit tieferer Suche (NMP) würden
   die meisten sichtbar — kein eigenständiger Eval-Fix nötig.

### Einschätzung vor NMP

Die „Stellschrauben anziehen"-Option vor NMP läuft auf zwei Kandidaten
hinaus — **Mobility** oder **Pawn-Endgame-Wissen**:

| Kandidat | Zielmotive | Aufwand | Risiko |
|---|---|---|---|
| **Mobility-Term** | unclassified (124), positional_collapse (34) = ~49% der Blunder | 1 neuer Eval-Term, Parameter-Tuning | Mittel — Mobility kann in geschlossenen Stellungen King-Aktivität überstimmen |
| **Pawn-Endgame-Guard** | endgame allows_mate (22) + endgame king_safety (19) = ~13% der Blunder, aber konzentriert | Opposition-Erkennung + Square-Rule | Hoch — spezialisiertes Endgame-Wissen ist fragil |
| **NMP** | indirekt alle Motive über tiefere Suche | Moderat, aber Zugzwang-Bugs im Endspiel berüchtigt | Hoch im Endspiel |

**Empfehlung:** Zuerst Mobility — der absolute Hebel (49% aller Blunder)
ist groß, der Effekt wirkt im Mittelspiel wo Martuni die meisten Partien
entscheidet, und der Term ist konservativ abschätzbar (kleine cp-Werte,
keine Tiefen-Pruning-Risiken wie bei NMP). Pawn-Endgame-Wissen bleibt
danach offen, weil es sauber isoliert vom Rest eingebaut werden kann.
NMP erst, wenn Mobility sich stabilisiert hat.

---

### Offene Schritte

5. **Mobility-Term in Eval** (nächster Schritt) — gegen `unclassified` +
   `positional_collapse` im Mittelspiel. Kleiner cp-Bonus pro legalem
   (Nicht-König-, Nicht-Bauer-)Zug, Piece-Typ-spezifisch, Tapered
   Midgame/Endgame.
6. **Pawn-Endgame-Guard** — Opposition in K+P-vs-K, Square-of-the-Pawn.
   Nach Mobility-Stabilisierung.
7. **SEE-Performance optimieren** — `all_attackers_to` könnte inkrementell
   aktualisiert werden statt pro Schlag neu berechnet.
8. **Eval-Erweiterungen (nach Daten):**
   - Outposts / schwache Felder
   - Turm auf 7. Reihe
   - Bishop-Trap-Detection
9. **Null-Move-Pruning** — erst wenn Mobility + Pawn-Endgame-Guard
   gemessen sind und Endspiel nicht mehr die größte Fehlerquelle ist.
