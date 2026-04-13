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

## Umsetzungsstatus (2026-04-12)

### Erledigt

1. **`see()` implementiert** in `search.rs` (Zeile ~594-760)
   - `see_piece_value()` — Materialwerte für SEE
   - `all_attackers_to()` — alle Angreifer auf ein Feld (inkl. X-Ray)
   - `least_valuable_attacker()` — billigsten Angreifer finden
   - `see()` — Hauptfunktion mit Gain-Array und Minimax-Rückfaltung
2. **Quiescence: Bad Capture Pruning** — `see(mv) < 0` → skip (Zeile ~444-447)

### Messergebnis (Stella-B-Position, 5s)

| Version    | Tiefe | Knoten | Zeit   | Zug  |
|------------|-------|--------|--------|------|
| Ohne SEE   | 2     | 3.4M   | 1826ms | Re8  |
| Mit SEE    | 2     | 2.5M   | 1422ms | Rd1  |
| Verbesserung | —  | **-27%** | **-22%** | —  |

SEE spart ~27% der Knoten bei gleicher Tiefe. Beide Versionen vermeiden
22...Nh5?? und 18...Bxd2??.

3. **Selektive Extensions mit SEE** — implementiert (2026-04-13): Captures in
   `is_candidate_move` nur noch extenden wenn `see(mv) >= 0`. Verlierende
   Schlagzüge bekommen keine +2-Halbzüge mehr — sie werden in der Quiescence
   ohnehin abgeschnitten. Netto: weniger Extensions, kein Mehraufwand.

### Offen (nächste Schritte)

4. **Move Ordering mit SEE** — zu teuer ohne Caching, zurückgestellt
5. **SEE-Performance optimieren** — `all_attackers_to` wird pro Schlagserie
   mehrfach aufgerufen; stattdessen inkrementell nur Gleiter-Angriffe
   aktualisieren
6. **Regression-Check** — Verlustpartien vor/nach SEE-Extensions mit
   `analyze_blunders.py` vergleichen
7. **Delta Pruning in Quiescence** — ergänzend zu SEE, unabhängig implementierbar
8. **Martuni-Eval in PGN prüfen** — `martuni_eval_cp` erscheint nie im Blunder-
   Report; klären ob lichess-bot die Eval als Variationsknoten speichert
9. **42 unclassified Blunder analysieren** — zweitgrößter Block, Motiv-Erkennung
   deckt ihn nicht ab; könnte blinde Flecken im Klassifizierer zeigen
