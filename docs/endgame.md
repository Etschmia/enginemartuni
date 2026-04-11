# Endspieltechnik ohne Tablebases

Konzept für hand-codiertes Endspielwissen, das die Engine ohne Syzygy- oder
Nalimov-Bases zu korrekten Ergebnissen in den wichtigsten elementaren
Endspielen führt. Entschieden: Umsetzung schrittweise in Phasen A → B → C,
alle Tuning-Parameter in einer `[endgame]`-Sektion in `eval.toml`.

## Grundidee

Klassische Engines (vor-TB-Ära) lösen Endspiele durch drei Bausteine:

1. **Material-Signatur erkennen** — Figurenzählung klassifiziert die Stellung
   (KvK, KRvK, KPvK, KBNvK, …) und schaltet auf ein spezielles Eval-Modul um.
2. **Spezifisches Endspielwissen als Eval-Term** (sog. *mop-up evaluation*):
   Gradienten bauen, die den Motor in die richtige Richtung ziehen, auch
   wenn die Suche das Matt selbst noch nicht sieht.
3. **Suchextensions im Endspiel**: Bei wenigen Figuren aggressiver verlängern,
   damit lange Mattsequenzen noch in die Suchtiefe passen.

## Architektur

Neues Modul `src/endgame.rs` mit einer Einstiegsfunktion

```rust
pub fn endgame_score(board: &Board, params: &EvalParams) -> Option<i32>
```

Sie liefert `Some(cp)` nur, wenn eine bekannte Material-Signatur erkannt
wird — sonst `None` und die normale Eval greift. Das Modul wird am Anfang
von `evaluate` abgefragt:

```rust
if let Some(s) = endgame::endgame_score(board, p) { return s; }
// ... sonst normale Eval ...
```

Alle Konstanten (Ecken-Gradient, König-Nähe-Bonus, Rule-of-the-Square-Bonus)
kommen in eine `[endgame]`-Sektion der `eval.toml`. Das bleibt konsistent mit
der restlichen Tuning-Philosophie.

## Phase A — Mop-up für erzwungene Matts

Behandelte Endspiele: **KRvK, KQvK, KRRvK, KQQvK**.

Ziel: der schwächere König wird an den Rand, dann in die Ecke getrieben;
der stärkere König kommt heran. Das Material-Plus macht das Endspiel
gewonnen — der Mop-up-Term liefert den Gradienten, damit die Engine das
Matt auch findet, wenn die Suche noch 30 Halbzüge entfernt ist.

**Eval-Terme**:

- `corner_distance(weaker_king)`: Chebyshev-Distanz zur nächsten der vier
  Ecken. Je näher, desto schlechter für den verteidigenden König.
  → `cp = mop_up_corner_weight * (7 - corner_distance)`
- `king_proximity`: Chebyshev-Distanz zwischen beiden Königen. Je näher,
  desto besser für den Angreifer.
  → `cp = mop_up_king_weight * (14 - 2 * king_distance)` o.ä.
- Zusammen ergibt das eine Landkarte, die die Engine unweigerlich ins
  Matt führt.

**Material-Erkennung**: Die stärkere Seite hat Material im Wert ≥ Turm
(Dame oder Turm oder mehr), die schwächere Seite hat nur den König. Alle
Bauern müssen verschwunden sein.

## Phase B — KPK mit Rule of the Square

Regel: ein Freibauer läuft durch, wenn der gegnerische König außerhalb des
*Quadrats* steht, das sich aus der Distanz zum Umwandlungsfeld ergibt.

**Formalisierung**: Sei `d = 8 - pawn_rank` (für Weiß) die Entfernung zum
Umwandlungsfeld. Der König muss innerhalb eines Quadrats der Seitenlänge
`d` erreichbar sein, sonst ist der Bauer nicht mehr einholbar.

```rust
fn inside_square(pawn_sq, pawn_color, king_sq, stm) -> bool
```

Wenn außerhalb → Bauer queent sicher → Bonus `passed_unstoppable_bonus`
(≈ +500 cp). Wenn innerhalb → normale Bewertung.

Ergänzend: Opposition erkennen (beide Könige in gerader oder ungerader
Distanz voneinander, auf derselben Linie/Reihe). Wer die Opposition hat,
bekommt einen Bonus; wer sie weichen muss, eine Strafe.

## Phase C — KBNK (Läufer + Springer)

Einziges reguläres Endspiel, das auch gute Spieler ärgert: Mattsetzen nur
in der Ecke der **Läuferfarbe** möglich. Mit normalem Mop-up läuft die
Engine im Kreis, weil sie in die "falsche" Ecke treiben würde.

**Lösung**: eigener Corner-Gradient, der nur zwei der vier Ecken anzieht —
die mit der Farbe des eigenen Läufers. Dazu:

```rust
let bishop_color = sq_color(bishop_sq);
let target_corners = if bishop_color == White { [A8, H1] } else { [A1, H8] };
```

Der Corner-Distance-Term verwendet die nächste dieser zwei Ecken.

Bonus: König-Nähe wie in Phase A. Ggf. zusätzlich ein Term, der den Springer
aktiv hält.

## Suchextensions

In allen drei Phasen darf die Suche aggressiver verlängert werden, sobald
die Material-Signatur erkannt ist. Einfach: wenn `endgame_score` nicht
`None` liefert, addiere +2 Halbzüge zum aktuellen Extension-Budget in
`search.rs`. Die Obergrenze bleibt der bestehende `MAX_EXTENSION_PER_LINE`.

Alternative: Check-Extensions freischalten, damit jedes Schachgebot
immer verlängert (nicht nur die ersten paar). Im Endspiel mit wenigen
Figuren ist das günstig.

## Reihenfolge der Umsetzung

1. Grundgerüst `src/endgame.rs` + `[endgame]`-Sektion in `eval.toml`
2. Phase A: KRvK + KQvK mit Mop-up-Eval → Testserie: matt in ≤ 25 Halbzügen
   aus zufälligen KQvK/KRvK-Stellungen
3. Phase B: KPK + Rule of the Square → Tests mit klassischen KPK-Lehrbuchstellungen
4. Phase C: KBNK → Testserie mit den berühmten Ausgangsstellungen

Pro Phase ein eigener Commit.

## Referenzen

- Chess Programming Wiki: [King-pawn endgames](https://www.chessprogramming.org/King_and_Pawn_versus_King_Endgame),
  [KBNK](https://www.chessprogramming.org/KBNK)
- Jeremy Silman, *Silman's Complete Endgame Course* — konzeptuelle Grundlagen
- Fruit/Toga-Sourcecode als Historienblick, wie Pre-TB-Engines das gelöst haben
