# Late Move Reductions (LMR) — Konzept für Martuni

**Status:** umgesetzt am 04.05.2026 in `src/search.rs`.
**Erwarteter Elo-Gewinn:** +30 bis +60 Elo (zu verifizieren nach ≥100 Lichess-Partien).
**Voraussetzung:** PVS — seit 01.05.2026 in `search.rs` drin.
**Betroffene Module:** ausschließlich `search.rs` (Move-Index aus `enumerate()` direkt nutzbar, keine Änderung an `order_moves` notwendig).

> **Warum jetzt:** Die 04.05.-Auswertung (175 Partien) zeigt, dass NMP wie
> erwartet `missed_mate` halbiert hat (0.105 → 0.057). Die verbleibenden
> Restfälle und der neue Hotspot `allows_mate` (0.126/P., 22 Fälle, fast
> ausschließlich in verlorenen Stellungen) sind strukturell Tiefen-Probleme:
> Martuni findet das schnellste Matt nicht, weil sie im PV-Pfad nicht tief
> genug rechnet. NMP hat die Knotenanzahl gesenkt — LMR ist die Mechanik,
> mit der wir die so gewonnene Suchzeit gezielt **in mehr Tiefe** umsetzen.

## Idee in einem Satz

Späte Züge in der Move-Ordering haben empirisch eine sehr geringe
Wahrscheinlichkeit, das beste Ergebnis zu liefern (sonst hätte Move-Ordering
sie weiter vorne einsortiert). Wir suchen sie deshalb mit **reduzierter
Tiefe** — und nur falls sie wider Erwarten gut aussehen, wird mit voller
Tiefe nachgesucht.

Während NMP einen kompletten Teilbaum überspringt, wenn die Stellung „zu
gut" ist, sucht LMR weiter — aber dünner. Beide Mechaniken ergänzen sich:
NMP killt langweilige *Stellungen*, LMR kürzt langweilige *Züge*.

## Ablauf innerhalb der Zugschleife

LMR sitzt in der normalen Move-Loop von `search`, **nach** der NMP-Prüfung
und **innerhalb** des PVS-Pfads. Pro Zug `i` (0-indexiert):

```
for (i, mv) in moves.iter().enumerate() {
    let new_board = board.make_move(mv);

    let score = if i == 0 {
        // Hauptlinie: volles Fenster, volle Tiefe (wie bisher in PVS)
        -search(new_board, depth - 1, -beta, -alpha, ...)
    } else {
        // Vorbedingungen für LMR prüfen — siehe unten
        let reduction = if can_reduce(board, mv, new_board, i, depth) {
            lmr_reduction(depth, i)   // 1 bis ~3 plies
        } else {
            0
        };

        // Reduzierte Nullfenster-Suche (wie PVS-Scout, aber flacher)
        let mut s = -search(new_board, depth - 1 - reduction, -alpha - 1, -alpha, ...);

        // Re-Search 1: wenn reduzierte Suche überraschend gut war,
        // mit voller Tiefe nochmal — immer noch Nullfenster.
        if s > alpha && reduction > 0 {
            s = -search(new_board, depth - 1, -alpha - 1, -alpha, ...);
        }

        // Re-Search 2: wenn auch das Fail-High war, volles Fenster
        // (Standard-PVS-Re-Search).
        if s > alpha && s < beta {
            s = -search(new_board, depth - 1, -beta, -alpha, ...);
        }

        s
    };

    // ... Cutoff-Logik wie bisher ...
}
```

Drei Schichten Re-Search wirkt erstmal viel, in der Praxis ist Layer 1
(Fail-High auf reduziert) selten und Layer 2 noch seltener. Der Gewinn
durch flache Suche bei den 80–90 % der Züge, die *nicht* hochgehen, deckt
das mehr als ab.

## Vorbedingungen — was NICHT reduziert werden darf

Das ist der kritischste Teil. Falsch parametrisiertes LMR führt direkt zu
`missed_capture`/`allows_mate`-Regressionen. Ein Zug `mv` wird **nur dann
reduziert**, wenn alle folgenden Bedingungen erfüllt sind:

1. **Move-Index ≥ Schwelle** — typisch ab `i ≥ 3` (also erste 3 Züge der
   Move-Ordering nie reduzieren). Begründung: TT-Move + Killer-Moves +
   gute Captures stehen vorne, dort lohnt Reduktion nicht.
2. **Tiefe ≥ 3** — bei `depth ≤ 2` sparen wir effektiv nichts und
   verlieren Genauigkeit.
3. **Kein Capture** (auch keine En-passant-Schläge).
4. **Keine Promotion** (zu taktisch, zu seltener Zug).
5. **Kein Schachgebot** (`new_board.checkers().popcnt() == 0`).
6. **Wir stehen selbst nicht im Schach** (`board.checkers().popcnt() == 0`).
   Im Schach ist jeder legale Zug erzwungen — Reduzieren wäre ein Bug.
7. **Kein Killer-Move** auf dieser Tiefe (falls Killer-Tabelle existiert).
8. **Optional: kein Pawn-Push auf 7. / 2. Reihe** — kurz vor Promotion ist
   praktisch immer taktisch relevant.

Optional, später: bei Stellungen mit `is_in_check_attack_threat` (eigener
König unter Druck, vom Gegner) reduktionsfrei lassen — Defensiv-Züge
brauchen volle Tiefe. Erst nach Mess-Ergebnis entscheiden.

## Reduktionsformel

Zwei sinnvolle Varianten:

**A — einfach, gut für den Einstieg:**
```
fn lmr_reduction(depth: i32, move_index: usize) -> i32 {
    if depth >= 6 && move_index >= 6 { 2 }
    else if depth >= 3 && move_index >= 3 { 1 }
    else { 0 }
}
```
Vorteil: leicht zu verstehen, leicht zu testen, wenig magische Zahlen.

**B — Stockfish-Stil (logarithmisch):**
```
fn lmr_reduction(depth: i32, move_index: usize) -> i32 {
    let r = (ln(depth as f32) * ln(move_index as f32) / 2.0).floor() as i32;
    r.max(0).min(depth - 2)   // nie tiefer reduzieren als die Suche selbst
}
```
Vorteil: passt sich glatt an Tiefe und Move-Index an. Nachteil: einmal
pro Zug `ln`-Aufruf, oder vorberechnete Lookup-Table (256×64 i8-Einträge,
~16 KB, einmalig in `lazy_static!` initialisiert).

**Empfehlung:** Variante A für den ersten Wurf, dann gegen Variante B
A/B-testen.

## Interaktion mit existierenden Mechanismen

- **PVS:** LMR sitzt im Nullfenster-Pfad, nicht beim ersten Zug. Erste
  Linie immer mit voller Tiefe — sonst wird die PV unzuverlässig.
- **NMP:** läuft *vor* LMR im selben Knoten. NMP entscheidet „kompletten
  Teilbaum schneiden", LMR entscheidet pro Zug „flacher suchen". Keine
  Konflikte.
- **Extensions:** unsere bestehenden Extensions (Schach-Extension +1,
  taktische Kandidaten) reduzieren effektiv die Tiefe **nicht** — wenn
  ein Zug schach gibt, ist Punkt 5 oben verletzt, also kein LMR. Wenn er
  ein gewinnender Capture ist, Punkt 3. Konsistent.
- **Quiescence:** unverändert — LMR endet vor dem Übergang in die
  Quiescence.

## Risiken und Symptome

| Risiko                                | Symptom in der Auswertung                    |
|---------------------------------------|----------------------------------------------|
| Reduktion zu aggressiv                | `missed_capture`/`allows_mate` steigt        |
| Defensiv-Züge geschnitten             | `exposed_king`/`positional_collapse` steigt  |
| Schach-/Capture-Filter fehlerhaft     | sporadische schwere Materialverluste         |
| Re-Search-Logik fehlerhaft            | Tiefen-Anzeige instabil, Knoten-Zahl steigt  |

Falls eines dieser Symptome im Mess-Match auftritt: Reduktion stufenweise
abschwächen (höhere `i`-Schwelle, niedrigeres `R`) oder zusätzliche
Vorbedingungen einziehen.

## Verifikations-Plan

**Stufe 1 — lokale Test-Stellungen** (vor Lichess-Deployment):
- Mittelspiel-Stellung aus dem NMP-Test (`r1bq1rk1/...`): erwartet weitere
  Knoten-Reduktion bei gleicher Tiefe, oder gleiche Knoten bei +1–2
  zusätzlich erreichten Tiefen-Plies.
- Mindestens drei verlorene Stellungen aus den 22 `allows_mate`-Fällen
  der 04.05.-Auswertung — Erwartung: schnellstes Matt wird gefunden.
- Mindestens drei taktische Stellungen, in denen ein gewinnender Capture
  *nicht* an Position 1 der Move-Ordering steht (sondern z. B. an 4–5).
  Erwartung: Re-Search erkennt den guten Zug; Engine spielt korrekt.

**Stufe 2 — Selbst-vs-Selbst** (wenn lokale Tests sauber):
- 200 Spiele bei kurzer TC (10+0.1 oder 8+0.08), abwechselnde Farben,
  fester Eröffnungs-Pool.
- Erfolgs-Kriterium: positive Elo-Differenz mit p<0.05, oder
  zumindest klares Plus ohne neue Blunder-Klassen.

**Stufe 3 — Lichess** (wenn Selbst-Match positiv):
- Rollout, ≥100 Partien sammeln.
- Auswertung mit `analyze_blunders.py --report` gegen den 04.05.-Stand.
- Ziel-KPIs:
  - `missed_mate`/Partie: 0.057 → ~0.03 (Halbierung der Restfälle)
  - `allows_mate`/Partie: 0.126 → ~0.07 (deutliche Reduktion)
  - Rating: weitere +30–60 Elo
- Sekundär: keine Verschlechterung bei `missed_capture`, `exposed_king`,
  `positional_collapse`.

## Eigenleistung — Aufgabenteilung

Was Tobias selbst macht:
- Reduktionsformel (A oder B, ggf. Tuning der Schwellen).
- Set der Vorbedingungen (welche der 8 Punkte oben aktiv, welche Reihenfolge).
- Bewertung der Mess-Ergebnisse und Entscheidung über Anpassungen.

Was Claude liefern darf (Infrastruktur):
- Re-Search-Skelett ins bestehende PVS einpassen.
- Move-Index sauber durch die Zugschleife reichen, falls noch nicht da.
- Ggf. die Lookup-Table für Variante B vorberechnen.
- Test-Skripte für die Verifikations-Stufe 1.

Engine-Logik bleibt Tobias' Entscheidung; Claude zeigt Optionen, Tobias
wählt.

## Klärungen vor der Implementierung (entschieden 04.05.2026)

1. **Killer-Moves** — vorhanden (`SearchState.killers`, `src/search.rs`,
   2 Slots pro Ply). In LMR integriert: ein Killer-Move wird nicht
   reduziert. Implementiert via `killers_here.iter().any(|k| *k == Some(mv))`.
2. **History Heuristic** — vorhanden (`SearchState.move_history`), aber
   bewusst **nicht** als zusätzliches LMR-Kriterium genutzt. Sie wirkt
   nur über die Zugreihenfolge in `order_moves`. Begründung: zwei
   überlappende Mechaniken auf dem gleichen Signal verschleiern, welche
   Komponente welchen Effekt liefert.
3. **Variante A vs. B** — Variante A (Stufenformel) als erster Wurf.
   Variante B (logarithmisch + Lookup-Table) bleibt im Plan, kommt erst
   nach Vermessung von A.
4. **Lookup-Table für Variante B** — nicht implementiert, gehört zu B.
5. **LMR im PV-Knoten** — nur in Non-PV-Knoten (`!is_pv`). PV-Knoten
   bleiben in Reichweite voller Suche unangetastet. Vorgemerkt für
   eine spätere Ausbaustufe (Stockfish reduziert auch PV-Knoten mit
   konservativeren Werten).

## Implementierungs-Notizen (04.05.2026)

**Bug-Falle, die wir bei der Verifikation gefunden haben:** Die Schutz-
Klausel `(new_depth - reduction).max(1)` darf **nur** greifen, wenn
tatsächlich reduziert wird (`reduction > 0`). Wenn man sie pauschal anwendet,
wird der reguläre Übergang `new_depth == 0` → Quiescence (an der Tiefen-
Grenze) zu `depth = 1` aufgebläht — jedes Blatt wird unnötig eine Ply tiefer
gerechnet, die Knoten explodieren, und keine Iteration der iterativen
Vertiefung schließt mehr ab. Symptom: `info string fallback (no completed
depth, nodes=…)` mit Millionen Knoten. Code im Repo:

```rust
let scout_depth = if reduction > 0 {
    (new_depth - reduction).max(1)
} else {
    new_depth
};
```

## Verifikation auf den Stufe-1-Stellungen

| Stellung                                                            | Pre-LMR                  | Post-LMR                   |
|---------------------------------------------------------------------|--------------------------|----------------------------|
| `4Q3/5p1k/6pp/4P3/1B1pBq2/8/1PP4P/4R2K w` — `mate 6` finden         | Tiefe 7, 17.4 M, 5.7 s   | **Tiefe 9, 6.8 M, 2.7 s**  |
| `6k1/6pp/p2p1r2/P3p1q1/1p1pPb2/3P2Pb/1PP1QP2/1R3RK1 w` (allows_mate Z. 132 der 04.05.-Analyse) — `Qf3` finden | Tiefe 7, 9.4 M, 4.9 s | **Tiefe 7, 3.1 M, 1.9 s** (Tiefe 8 in 3.2 s) |

Beide Erwartungen aus dem Konzept erfüllt: erstes Beispiel zeigt
**+2 Plies in deutlich weniger Zeit**, zweites zeigt **−67 % Knoten**
auf gleicher Tiefe und korrekten Zug. Cargo-Tests (63) grün, NMP-
Verifikation aus dem 01.05.-Release weiterhin sauber (`mate 6` wird
früher gefunden, nicht später).

Stufen 2 (Selbst-vs-Selbst) und 3 (Lichess-Auswertung nach ≥100 Partien)
stehen aus.
