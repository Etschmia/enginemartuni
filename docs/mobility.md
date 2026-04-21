# Mobility in der Eval

Dieses Dokument erklärt, **was Mobility ist**, warum Martuni einen Mobility-Term
braucht und **welche Varianten** wir implementieren können. Es ist — wie
`see.md` — als Lern-Referenz für Tobias geschrieben, mit Optionen und
Empfehlung, **ohne den Code schon zu ändern**.

## Warum überhaupt Mobility?

Die 20.04-Analyse (322 Blunder / 154 Partien) zeigt:

- **`unclassified` im Mittelspiel: 97 Fälle** — die mit Abstand größte
  einzelne Kategorie.
- **`positional_collapse`: 34 Fälle** insgesamt, 20 im Mittelspiel.

Zusammen sind das **~49% aller Blunder**. Beide Motive haben keinen klaren
taktischen Punkt (kein Hänger, kein Matt) — sie tauchen auf, wenn Martuni
eine "unaufgeräumte" Stellung wählt:

- Türme, die auf Grundreihen stehen und nicht in die offenen Linien kommen.
- Springer auf Randfeldern ohne gute Felder.
- Läufer, die vom eigenen Bauerngerüst blockiert sind.
- Damen, die nur Fluchtfelder haben.

Die aktuelle Eval "sieht" solche Stellungen als fast ausgeglichen, weil sie
nur Material + PST + ein paar Struktur-Boni bewertet. Eine Figur auf a1 und
dieselbe Figur auf d4 bekommen denselben PST-Wert — die Zahl ihrer legalen
Züge fließt nirgends ein.

Beispiele aus der Analyse:

- `Jibbby vs Martuni - 6FbYcvgm` — 7 unclassified-Einträge in einer Partie,
  alle mit `best=Rd3` oder `best=Rfd8`. Der beste Zug laut SF war wiederholt
  **den Turm auf eine aktive Linie zu bringen** — Martuni wählte h6, a4, Be3,
  Rfe8 usw. weil nichts in seiner Eval die passive Turmstellung bestraft.
- `Martuni vs GarboBot - d1CXtcep` — 9 Blunder, mehrere davon `Qe3/Ra3/Bb3`
  statt aktiver Turm-Züge.
- `Martuni vs sseh-c - 6BGqkEO6/6ODn8e1j` — `Bc2/Be2/Bb3/f5/f3` Züge wo SF
  immer Figur-Aktivierung vorschlägt.

## Was macht ein Mobility-Term?

Die einfachste Antwort:

> **Pro legalem (oder pseudo-legalem) Zug einer Figur gibt es einen kleinen
> cp-Bonus. Piece-Typ-spezifisch, im Endspiel meist höher als im Mittelspiel.**

Das belohnt indirekt genau das, was wir wollen:

- Ein Turm auf offener Linie hat ~11 Züge → viel Bonus.
- Ein Turm auf a1 mit Bauern davor hat 0 Züge → kein Bonus.
- Ein Springer auf d5 hat 8 Züge → viel Bonus.
- Ein Springer auf a1 hat 2 Züge → kaum Bonus.

## Drei Varianten (zunehmend komplex)

### Variante A — "Naive Mobility"

Pro Figur: `count(pseudo-legale Zielfelder, die nicht von eigener Figur besetzt sind) × bonus[piece_type]`.

- **Pseudo-legal** heißt: wir prüfen nur geometrische Zugmöglichkeit (bei
  Gleitern inkl. Blocker), aber nicht, ob der König nachher im Schach steht.
- Das crate liefert `get_rook_moves`, `get_bishop_moves`, `get_knight_moves`,
  `get_king_moves` — alle mit Occupancy. Schlagzüge sind enthalten.

**Vorteil:** sehr schnell (nur Bitboard-Popcount), keine `MoveGen`-Aufrufe.
**Nachteil:** zählt auch Züge, die die Figur in einen gegnerischen
Bauernangriff stellen würden → bewertet "schlechte" Mobility gleich hoch
wie "gute".

### Variante B — "Safe Mobility"

Wie A, aber Zielfelder in gegnerischen Bauernangriffen werden **nicht
mitgezählt**.

- Eine zusätzliche BitBoard-Maske `enemy_pawn_attacks` pro Seite (1× pro Eval
  berechnet), dann `legal_targets & !enemy_pawn_attacks`.
- So wird ein Springer auf f3, der nach e5 ziehen könnte aber dort vom
  gegnerischen Bauern geschlagen würde, nicht belohnt.

**Vorteil:** bewertet nur "echte" Mobility, schneidet die größte
Fehlerquelle von Variante A ab.
**Nachteil:** minimal mehr Rechenzeit — eine BitBoard-Oder pro Seite.

### Variante C — "Attack-aware Mobility"

Wie B, aber auch Felder, die von stärkeren gegnerischen Figuren angegriffen
werden und nicht ausreichend gedeckt sind, werden abgezogen (quasi ein
light-SEE auf dem Zielfeld).

**Vorteil:** am präzisesten.
**Nachteil:** teuer — wir würden für jedes Zielfeld eine Art Mini-SEE
berechnen. In der Eval (die pro Blattknoten ausgeführt wird) ist das
heikel.

## Empfehlung

**Variante B ("Safe Mobility")** als erster Schritt. Kostet ~1% mehr Eval-Zeit
gegenüber A, hat aber den größten Teil des qualitativen Gewinns. Variante C
hebe ich mir auf, falls sich später zeigt, dass Martuni gerne Figuren "in
Deckungen" zieht die nur gegen Bauern safe sind.

Konkret für Variante B:

```
fn mobility(board, us, params) -> i32:
    enemy_pawn_attacks = pawn_attacks_of(board, !us)   // 1× pro Seite
    safe_mask = !(our_pieces) & !enemy_pawn_attacks

    let mut mg = 0
    let mut eg = 0

    für jeden Springer von us:
        moves = get_knight_moves(sq) & safe_mask
        mg += popcnt(moves) * knight_mg_weight
        eg += popcnt(moves) * knight_eg_weight

    für jeden Läufer von us:
        moves = get_bishop_moves(sq, occ) & safe_mask
        mg += popcnt(moves) * bishop_mg_weight
        eg += popcnt(moves) * bishop_eg_weight

    für jeden Turm von us:
        moves = get_rook_moves(sq, occ) & safe_mask
        mg += popcnt(moves) * rook_mg_weight
        eg += popcnt(moves) * rook_eg_weight

    für jede Dame von us:
        moves = (get_rook_moves(sq, occ) | get_bishop_moves(sq, occ)) & safe_mask
        mg += popcnt(moves) * queen_mg_weight
        eg += popcnt(moves) * queen_eg_weight

    return taper(mg, eg, phase)
```

**Was nicht mitgezählt wird:**

- Bauern (Zugmuster ist festgelegt, PST + Freibauer-Bonus decken die Aktivität ab).
- König (schon über King-Safety und `king_activity_endgame` bewertet).
- Springer-Züge auf gegnerische Figuren bleiben drin (das ist echte
  Mobility: Druck ausüben).

## Parameter-Vorschläge (Startwerte zum Tunen)

Diese Zahlen sind **konservativ**, damit Mobility nicht die bestehenden
Terme überstimmt:

| Figur   | mg (cp/Zug) | eg (cp/Zug) | Begründung |
|---------|-------------|-------------|------------|
| Knight  | 3           | 3           | Maximal 8 Züge → ±24cp, symmetrisch mg/eg. |
| Bishop  | 3           | 4           | Bis 13 Züge; Läufer im Endspiel besonders wertvoll. |
| Rook    | 2           | 5           | Bis 14 Züge; Türme entfalten sich erst im Endspiel. |
| Queen   | 1           | 2           | Bis 27 Züge; Dame wird sonst überbewertet. |

Max-Größenordnungen:
- MG: Knight 24, Bishop 39, Rook 28, Queen 27 → Summe pro Seite in
  wilden Stellungen ca. 100–120 cp.
- EG: ähnlich, Rook/Queen etwas höher.

Das ist deutlich mehr als einzelne bestehende Terme (Rook-Open-File = 30)
aber weit unter Figurenwerten. In typischen Mittelspielstellungen dürften
Differenzen zwischen beiden Seiten bei 20–60 cp liegen — spürbar, nicht
dominant.

## Kostenabschätzung

Eval wird pro Blattknoten einmal aufgerufen. Zusätzlicher Aufwand pro
Aufruf:

- 1× `enemy_pawn_attacks` pro Seite (2× popcnt/shift, billig).
- Pro Springer/Läufer/Turm/Dame genau ein `get_X_moves`-Lookup plus einen
  AND-popcnt.
- Insgesamt bei 4 Springern + 4 Läufern + 4 Türmen + 2 Damen = ~14
  Lookups. Jeder ist sub-µs.

Erwarteter Overhead: **1–3% NPS-Verlust**. Das sollte sich in der Spielstärke
mehrfach auszahlen.

## Implementierungsplan (falls Variante B genommen wird)

1. **`eval_config.rs`**: 8 neue Parameter `knight_mg_mobility`, `knight_eg_mobility`,
   …, `queen_eg_mobility`. Defaults wie Tabelle oben. TOML-Sektion
   `[mobility]`.
2. **`eval.rs`**:
   - Neue Funktion `pawn_attacks(bitboard, color) -> BitBoard` (shift
     N/NE/NW bzw. S/SE/SW).
   - Neue Funktion `mobility_score(board, us, params) -> (mg, eg)`.
   - In `evaluate()` pro Seite aufrufen, `(w_mg, w_eg)` − `(b_mg, b_eg)`
     durch `taper()` schicken und zum Endscore addieren.
3. **`eval.toml`**: `[mobility]`-Sektion mit Defaults und Erklärkommentaren
   ergänzen, analog zu `[pieces]`.
4. **Tests**: ein simpler Test mit einer Stellung, in der die Mobility-Zahl
   eindeutig ist (z.B. Springer im Zentrum vs Springer in der Ecke), plus
   Anpassung der bestehenden Tests, wo der Gesamtscore durch Mobility
   verschoben wird.
5. **Smoke-Test**: `cargo test` grün + Kiwipete 3s gegen Stella-B /
   berlinschach-GUI kurz ausprobieren, Startwert und Bestzug plausibel.
6. **Messfenster**: ein paar Tage laufen lassen, danach Blunder-Analyse.

## Risiken und was wir beobachten

- **King-Mobility fehlt bewusst** — sonst würden wir dem König im Endspiel
  doppelt Aktivitätsbonus geben (wir haben schon `king_activity_endgame`).
- **Mobility kann den Bishop-Pair-Bonus relativieren** — zwei Läufer haben
  in offenen Stellungen automatisch mehr Mobility. Das ist ok, aber wir
  sollten den Bishop-Pair-Bonus beobachten und ggf. leicht reduzieren.
- **Mobility kann King-Safety untergraben** — ein offener König hat viele
  "Mobility"-Züge, obwohl er unsicher steht. Da wir King-Moves **nicht**
  mitzählen, entfällt dieses Risiko.
- **Im Endspiel kann Mobility den Turmaktivierung-Bonus (Open File)
  verdoppeln** — das ist ok, beide messen dasselbe und ein Turm auf offener
  Linie soll besser stehen.

## Offene Entscheidungen für Tobias

1. **Variante A, B oder C?** (Empfehlung: B.)
2. **Mobility auch für Bauern?** (Empfehlung: nein — PST reicht.)
3. **Mobility auch für König?** (Empfehlung: nein — würde mit
   `king_activity_endgame` doppeln.)
4. **Tapered (mg/eg getrennt) oder einheitlich?** (Empfehlung: getrennt —
   Türme/Damen skalieren unterschiedlich nach Phase.)
5. **Parameter-Startwerte wie oben?** Oder lieber noch zahmer beginnen
   (alles /2) und bei Bedarf hochfahren?
6. **"Safe Mask"**: nur gegen Bauernangriffe, oder auch eigene Figuren
   ausschließen? (Empfehlung oben: beides, wie im Pseudocode. Eine Figur
   kann nicht auf das Feld einer eigenen Figur ziehen — das ist keine
   Mobility.)

Wenn du die Variante und die Parameter-Startwerte freigibst, baue ich das
ein und melde mich mit `cargo test` + Smoke-Test zurück.
