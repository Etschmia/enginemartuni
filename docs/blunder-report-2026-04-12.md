# Blunder-Report: 14 Verlustpartien vom 11./12. April 2026

Analysiert mit `tools/analyze_blunders.py`, Stockfish bei 1 s/Zug, Threshold 100 cp.
PGN-Quelle: `pgn/Martuni_lost.pgn` (Lichess-BOT-Partien, Blitz + Rapid).

## Zusammenfassung

**103 Blunder** in 14 Partien (nur Martunis Züge).

### Nach Phase

| Phase       | Anzahl | Anteil |
|-------------|--------|--------|
| Mittelspiel | 70     | 68 %   |
| Endspiel    | 27     | 26 %   |
| Eröffnung   |  6     |  6 %   |

### Nach Motiv

| Motiv                 | Anzahl | Was es bedeutet                                    |
|-----------------------|--------|----------------------------------------------------|
| unclassified          | 40     | Positionelle Fehler ohne erkennbares takt. Muster  |
| allows_mate           | 26     | Zug ermöglicht Matt für den Gegner                 |
| hangs_bishop          | 13     | Läufer wird eingestellt (hängt nach dem Zug)       |
| king_safety           | 13     | Königssicherheit verschlechtert sich drastisch     |
| hangs_knight          |  9     | Springer eingestellt                               |
| missed_capture        |  8     | Besserer Schlagzug war verfügbar                   |
| positional_collapse   |  6     | Großer Eval-Verlust ohne taktisches Motiv (≥300cp) |
| hangs_rook            |  3     | Turm eingestellt                                   |
| missed_mate           |  1     | Eigenes Matt-in-N nicht gespielt                   |
| hangs_queen           |  1     | Dame eingestellt                                   |

### Phase × Motiv (Top-Cluster)

| Phase       | Motiv                | Anzahl |
|-------------|----------------------|--------|
| Mittelspiel | unclassified         | 32     |
| Endspiel    | allows_mate          | 15     |
| Mittelspiel | hangs_bishop         | 11     |
| Mittelspiel | allows_mate          | 11     |
| Endspiel    | king_safety          |  8     |
| Mittelspiel | positional_collapse  |  6     |
| Mittelspiel | king_safety          |  5     |

## Auffällige Einzelfehler

### 18...Bxd2?? (vs. melsh_bot, Rapid 15+10)

- Partie: `Martuni_lost.pgn#1`, Schwarz
- Loss: 291 cp, Motiv: `hangs_bishop`
- Stockfish empfiehlt: `c4+`
- Martuni tauscht den Läufer gegen einen Bauern, übersieht den Rückschlag.

### 22...Nh5?? (vs. Stella-B, Rapid 15+10)

- Partie: `Martuni_lost.pgn#9`, Schwarz
- Loss: 557 cp, Motiv: `hangs_knight`
- Stockfish empfiehlt: `b6`
- Springer geht nach h5, wo er von der Dame geschlagen wird. Einzügiger Einsteller.

### 14...Nxc2+?? (vs. melsh_bot, Rapid 15+10)

- Partie: `Martuni_lost.pgn#1`, Schwarz
- Loss: 99381 cp (verpasstes Matt!), Motiv: `missed_mate`
- Stockfish empfiehlt: `Qg5+` (Matt-Angriff)
- Martuni gewinnt zwar Material, verpasst aber ein forciertes Matt.

### 18. Bxf5?? (vs. MateMakingMachine, Blitz 3+2)

- Partie: `Martuni_lost.pgn#11`, Weiß
- Loss: 246 cp, Motiv: `hangs_bishop`
- Stockfish empfiehlt: `Rab1`
- Läufer schlägt auf f5, wird aber dort eingestellt.

## Diagnose: Gemeinsame Ursache

Die taktischen Fehler (hangs_* + missed_capture + allows_mate = **57 von 103 Blundern**)
haben eine gemeinsame Wurzel: Martuni kann Abtauschserien nicht statisch bewerten.

Es fehlt eine **Static Exchange Evaluation (SEE)** — siehe `docs/see.md`.

Ohne SEE:
1. Quiescence durchsucht ALLE Schlagzüge, auch offensichtlich verlierende → Zeitverschwendung
2. Alle Captures bekommen pauschal +2 Extension → Suchbaum explodiert
3. Keine Delta-Pruning → hoffnungslose Captures werden voll expandiert
4. Effektive Suchtiefe sinkt → einfache Taktik bleibt unsichtbar

Maßnahmenplan: siehe `docs/see.md` (Erklärung + Implementierungsentscheidungen).

## Offene Baustellen (nicht in diesem Schritt)

Die 40 `unclassified` Blunder sind positionelle Schwächen (fehlende Eval-Features):
- Freibauern-Erkennung in der Eval
- Rook on 7th / offene Linien
- Schwache Felder / Outposts
- Bishop Pair

Diese werden separat angegangen, nachdem die taktische Basis steht.
