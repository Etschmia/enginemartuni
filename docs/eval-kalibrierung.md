# eval.rs — Kalibrierung und bekannte Problemstellen

Dieses Dokument hält fest, welche Eval-Terme wir warum angepasst haben,
welche Werte sich bewährt haben und wo noch Verbesserungspotenzial liegt.

## Architektur (Kurzfassung)

```
evaluate(board) = non_pst + taper(pst_mg, pst_eg, phase)
```

- **`non_pst`**: Material + Figurboni/Strafen + Bauernstruktur + King Safety
  (alle phasenflach, d.h. gleiches Gewicht Mitte- und Endspiel)
- **`pst`**: Piece-Square-Tables, via `taper()` zwischen MG und EG interpoliert
- **`endgame_score`**: spezialisierte Endspielmodule (KPvK, KRvK, …) übernehmen
  die Bewertung komplett wenn erkannt

## Freibauern-Bonus: Redesign (2026-04-13)

### Problem: `pawn_passed_bonus = 300cp` war zu hoch

Ursprünglich: ein einzelner flacher Bonus von 300cp für jeden Freibauern,
unabhängig davon, wie weit der Bauer vorgerückt ist.

**Folge:** Nach einem Figurenopfer, das den gegnerischen a-/b-Bauern vom Brett
räumt, klassifiziert die Eval den eigenen a2-Bauern plötzlich als Freibauer
(+300cp). Kombiniert mit dem Materialverlust von −200cp bleibt ein scheinbar
positives Ergebnis — die Engine opfert die Figur, weil der Rückschlag von der
Quiescence nicht gesehen wurde (→ SEE-Bug, siehe `docs/see.md`).

Auch nach Behebung des SEE-Bugs blieb das Problem bestehen: ein a2-Freibauer
im Mittelspiel ist kaum gefährlich, weil Stücke ihn trivial blockieren. +300cp
ist ungefähr der Wert eines Springers — völlig unrealistisch für Rang 2.

### Lösung: Rangabhängiger Bonus (`pawn_passed_rank_bonuses`)

Neues Feld in `EvalParams`:

```rust
pawn_passed_rank_bonuses: Vec<i32>
// Index = Vormarsch-Rang: 0 = Ausgangsreihe, 5 = ein Schritt vor Umwandlung
```

Standardwerte:

| Advancement | Rang (Weiß) | Bonus |
|---|---|---|
| 0 | Reihe 2 (a2) | **5 cp** |
| 1 | Reihe 3 | 15 cp |
| 2 | Reihe 4 | 30 cp |
| 3 | Reihe 5 | 55 cp |
| 4 | Reihe 6 | 100 cp |
| 5 | Reihe 7 (a7) | **170 cp** |

**Ergebnis:** Die blunder-Position `rds8gwiN` Zug 14 spielt die Engine nach
dem Fix korrekt **c4** (Stockfishs Empfehlung) statt Bxa7.

### Warum kein Taper (MG/EG)?

Wäre theoretisch sauberer, aber der rangabhängige Bonus approximiert das
bereits ausreichend: kleine Boni auf frühen Rängen entsprechen "Mittelspiel-
Einschätzung", hohe Boni auf späten Rängen entsprechen "Endspiel-Einschätzung".
Expliziter MG/EG-Split kann folgen wenn nach weiteren Partien Bedarf erkennbar.

### Konfigurierbar via eval.toml

```toml
[pawns]
passed_rank_bonuses = [5, 15, 30, 55, 100, 170]
```

---

## King Safety: bekannte Schwäche

Der `pawn_shield_score` greift nur wenn der König noch auf der Grundreihe steht
(`king_rank != home_rank → return 0`). Im Endspiel ist das korrekt (aktiver
König). Im Mittelspiel kann ein nach g1 castlierter König mit vorgerückten
Bauern diesen Bonus verlieren sobald er sich um einen Schritt bewegt. Bisher
kein konkreter Schaden beobachtet, merken für spätere Kalibrierung.

---

## Offene Eval-Baustellen

1. **sf_diff-Überschätzen** — vereinzelt beobachtet (v7p3r_bot 2026-04-13):
   Martuni +800–966cp, SF sah +80–430cp. Ursache unklar nach SEE-Fix. Erneut
   prüfen wenn neue Partien vorliegen.

2. **Freibauern-Bonus tapen** — expliziter MG/EG-Split wenn Spielmaterial das
   erfordert (z.B. Endspielpartien zeigen Regressionsprobleme).

3. **Isolierte Bauern im Endspiel** — Strafe von −20cp ist phasenflach; im
   Endspiel sind isolierte Bauern gravierender.

4. **Springer vs. Läufer** — keine spezifische Bewertung (offene vs. geschlossene
   Stellung), beide uniform 300cp.
