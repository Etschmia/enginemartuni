# Null-Move-Pruning (NMP) — Konzept für Martuni

**Status:** umgesetzt am 01.05.2026 (zusammen mit PVS, siehe unten).
**Erwarteter Elo-Gewinn:** +50 bis +80
**Betroffene Module:** `search.rs` (primär), ggf. `eval.rs` (Zugzwang-Erkennung)

> **Befund 01.05.2026 — PVS-Voraussetzung:** Bei der Erst-Implementierung
> stellte sich heraus, dass NMP in Martuni nie greift, weil unsere Suche
> bisher kein Principal Variation Search (PVS) nutzte. Ohne PVS wird jeder
> rekursive Aufruf mit dem vollen Fenster `(-beta, -alpha)` gemacht — die
> Vorbedingung `!is_pv` (`beta - alpha > 1`) ist dann praktisch nie erfüllt.
> Lösung: PVS gleich mitgeliefert. Mit PVS werden alle Knoten außer der
> Hauptlinie zu Nullfenster-Knoten, NMP greift natürlich.
>
> Verifikation an `4Q3/5p1k/6pp/4P3/1B1pBq2/8/1PP4P/4R2K w - - 0 29` (eine
> `missed_mate`-Stellung aus dem 01.05-Analyse): vorher Tiefe 4 / cp 1534 in
> 8 s, kein Matt; nach NMP+PVS Tiefe 7 / `mate 6` in <6 s. Mittelspiel-
> Test-Stellung: −43 % Knoten auf gleicher Tiefe.

## Idee in einem Satz

Wenn unsere Stellung so gut aussieht, dass selbst ein geschenktes Tempo für den Gegner die Bewertung nicht unter `beta` drückt, dürfen wir den ganzen Teilbaum abschneiden, ohne ihn regulär zu durchsuchen.

## Ablauf

Innerhalb von `search(depth, alpha, beta, ...)`, **bevor** die normale Zugschleife startet:

1. **Vorbedingungen prüfen** (alle müssen erfüllt sein — siehe unten).
2. **Null Move ausführen:** Seite am Zug wechseln, En-passant-Feld löschen, Halbzugzähler inkrementieren. *Kein* Figurenzug.
3. **Reduzierte Suche:** `score = -search(depth - 1 - R, -beta, -beta + 1, ...)` — Nullfenster, weil wir nur wissen wollen, ob `score ≥ beta`.
4. **Null Move zurücknehmen.**
5. **Entscheidung:**
   - `score ≥ beta` → **Cutoff:** `return beta` (bzw. `score`, je nach Fail-Soft/Hard-Konvention).
   - sonst → normale Suche wie gehabt, kein Pruning.

## Parameter

- **R (Reduction):** Standard `R = 2`; in modernen Engines oft adaptiv, z.B. `R = 2 + depth/6`. Für Martuni starten wir mit konstantem `R = 2`, später evtl. `R = 3` bei hoher Tiefe.
- **Mindesttiefe:** erst ab `depth ≥ 3` anwenden (bei `depth ≤ 2` landet man direkt in der Quiescence, spart nichts).

## Vorbedingungen (Sicherheitsnetze)

Alle müssen erfüllt sein, sonst **kein** Null Move:

| Bedingung | Warum |
|---|---|
| Seite am Zug **nicht im Schach** | Null Move wäre illegal. |
| **Kein** direkt vorangegangener Null Move | Zwei hintereinander = sinnlos, führt zu falschen Cutoffs. |
| `depth ≥ 3` | Darunter kein Nutzen. |
| **Nicht in der PV** (non-PV-Node, d.h. `beta - alpha == 1`) | NMP ist ein Pruning-Trick und darf die Hauptvariante nicht verfälschen. |
| **Static Eval ≥ beta** | Nur wenn die Stellung *jetzt schon* gut aussieht, lohnt der Test. |
| **Kein Zugzwang-Risiko** | Siehe unten — der wichtigste Punkt. |

## Zugzwang-Problem

Die NMP-Annahme lautet: *"Einen Zug zu machen ist mindestens so gut wie zu passen."* In Zugzwang-Stellungen ist das **falsch** — jeder legale Zug verschlechtert die Lage. NMP würde dort fälschlich Cutoffs auslösen und die Engine spielt blind in verlorene Stellungen.

**Pragmatische Erkennung (Standard-Heuristik):**

- Seite am Zug hat **nur König und Bauern** → NMP deaktivieren.
- Alternativ strenger: Seite am Zug hat **keine Leichtfiguren oder Schwerfiguren** (nur King + Pawns + evtl. Gegner-Figuren zählen nicht) → deaktivieren.

Für Martuni empfehle ich die einfache Variante: NMP aus, sobald die Seite am Zug keine Offiziere mehr hat. Das deckt 95% der Zugzwang-Fälle in Bauernendspielen ab, bei minimalem Implementierungsaufwand.

## Integration in die bestehende Suche

Pseudocode-Skizze für `search.rs`:

```rust
fn search(pos, depth, alpha, beta, ply, allow_null, ...) -> i32 {
    // ... existing: TT-Probe, Terminal-Check, Quiescence bei depth == 0 ...

    let in_check = pos.in_check();
    let is_pv = beta - alpha > 1;

    // --- Null-Move-Pruning ---
    if allow_null
        && !is_pv
        && !in_check
        && depth >= 3
        && has_non_pawn_material(pos, pos.side_to_move())
        && static_eval(pos) >= beta
    {
        let r = 2;
        pos.make_null_move();
        let score = -search(pos, depth - 1 - r, -beta, -beta + 1, ply + 1,
                            /* allow_null = */ false, ...);
        pos.unmake_null_move();

        if score >= beta {
            return beta; // Cutoff
        }
    }

    // ... existing: normale Zugschleife ...
}
```

**Wichtig:** Der `allow_null`-Flag verhindert zwei Null Moves hintereinander. Beim rekursiven Aufruf innerhalb von NMP → `false` übergeben, sonst `true`.

## Null Move auf dem `chess`-Crate

Das `chess`-Crate hat von Haus aus **keine** `make_null_move`-Funktion, da Null Move kein legaler Schachzug ist. Lösungsoptionen:

1. **Eigener `Position`-Wrapper mit Null-Move-Unterstützung:** Im `position.rs`-Modul eine Methode ergänzen, die intern einen neuen `Board` mit geflippter `side_to_move` und geleertem En-passant-Feld baut. `chess::Board` hat `null_move()` bereits — gibt `Option<Board>` zurück (None, wenn im Schach). Das ist der saubere Weg.
2. **Manuelles Zobrist-Update** für die TT — wichtig, damit die TT nach Null Move nicht falsche Treffer liefert. Der Side-to-Move-Key muss geflippt werden, En-passant-Key entfernt.

Empfehlung: Option 1, `chess::Board::null_move()` direkt verwenden.

## Verifikation

Nach der Implementierung:

1. **Selbsttest Alt vs. Neu** (20-30 Partien, kurze Bedenkzeit) — NMP sollte klar besser sein, sonst stimmt was nicht.
2. **Taktik-Suite** (z.B. WAC, STS) — Gesamt-Score sollte steigen oder gleich bleiben; Lösungszeiten deutlich fallen.
3. **Spezialtest Zugzwang-Stellungen:** Manuell ein paar bekannte Zugzwang-Positionen (King+Pawn-Endspiele) testen, dass Martuni den richtigen Zug findet und nicht durch NMP-Cutoff daneben liegt.

## Ausblick

Wenn Basis-NMP läuft und Elo-Gewinn bestätigt ist, mögliche Erweiterungen:

- **Adaptive Reduction:** `R = 2 + depth/6`.
- **Verification Search:** Bei hoher Tiefe den Cutoff mit einer zweiten, flacheren, *regulären* Suche gegenprüfen (schützt zusätzlich vor Zugzwang-Fehlern).
- **Double Null Move Extension:** In sehr späten Endspielen NMP komplett ausschalten statt nur bei "kein Offizier".

Diese Verfeinerungen erst nach stabiler Basis-Version.

## PVS — Principal Variation Search (mitgeliefert 01.05.2026)

PVS ist die unverzichtbare Voraussetzung dafür, dass NMP überhaupt wirken
kann (siehe Befund oben). Idee in einem Satz:

> Den ersten Zug (durch Move-Ordering vermutlich der beste) mit vollem
> Fenster prüfen, alle weiteren mit einem Nullfenster nur darauf testen,
> ob sie diesen Anker schlagen — und nur bei Überraschung mit vollem
> Fenster nachsuchen ("Re-Search").

Implementierung in `search.rs` (Move-Schleife):

```rust
let score = if first_move {
    -alpha_beta(&nb, new_depth, ply+1, -beta, -alpha, ...)        // voll
} else {
    let scout = -alpha_beta(&nb, new_depth, ply+1, -alpha-1, -alpha, ...);  // Nullfenster
    if scout > alpha && scout < beta {
        -alpha_beta(&nb, new_depth, ply+1, -beta, -alpha, ...)    // Re-Search
    } else {
        scout
    }
};
```

Warum das funktioniert: Alpha-Beta wird umso wirksamer, je enger das
Fenster ist. Ein Nullfenster `[alpha, alpha+1]` lässt nur die Antwort zu
"besser oder schlechter als alpha?" — viele Pruning-Möglichkeiten greifen
sofort, die mit vollem Fenster nicht greifen würden. Bei guter
Move-Ordering ist der erste Zug fast immer der beste, also brauchen die
restlichen Züge nur einen *Test*, keine vollständige Bewertung.

**Re-Search-Risiko:** Wenn die Move-Ordering schlecht ist, müssen viele
Züge re-searched werden, und der Aufwand kippt ins Negative. Martunis
Ordering (TT-Move > Promotion-Dame > gewinnender Capture > Killer >
Quiet/History > verlierender Capture) ist solide genug, dass das nicht
auftritt — die Messung am 01.05.2026 zeigt durchweg Knoten-Reduktion.

**Bonus:** PVS allein bringt typischerweise +20–40 Elo, unabhängig von
NMP. Mit NMP zusammen erwarten wir den im Doku oben genannten
Gesamt-Gewinn.
