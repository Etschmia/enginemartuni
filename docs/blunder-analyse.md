# Blunder-Analyse: Martunis Fehler zum Lernsignal machen

Dieses Dokument beschreibt, **wofür** `tools/analyze_blunders.py` existiert, wie
das Skript funktioniert und **wie die Ergebnisse zurück in den Code fließen**.
Es ist die Brücke zwischen "Martuni hat verloren" und "wir wissen jetzt, an
welcher Stellschraube zu drehen ist".

## Grundidee

Verlustpartien enthalten mehr Information als ELO-Zahlen. Wenn Stockfish Zug
für Zug sagt, wo der centipawn-loss passiert ist, kann man die Fehler clustern
und Muster sehen:

- *"60 % der Punkte gehen im Endspiel verloren"* → Endspielmodul ausbauen
- *"Horizon-Effekt bei ruhigen Stellungen mit Mattdrohung"* → Move Ordering / Suchextensions
- *"Verpasst Bauernketten-Hebel"* → positionelles Wissen in der Eval fehlt
- *"Hängt regelmäßig Figuren im Mittelspiel"* → King-Safety-Gewicht oder Quiescence

Zehn bis dreißig Verlustpartien reichen für erste belastbare Muster.

## Workflow

```
PGN (Lichess / berlinschach / lokale Tests)
   ↓
analyze_blunders.py  --player Martuni  --threshold 150  --movetime 0.3
   ↓
gruppierter Report: Phase × Motiv × Häufigkeit (nur Martunis Züge!)
   ↓
manuelle Interpretation → Änderung in eval.toml / eval.rs / search.rs
   ↓
neue Partien spielen → wieder analysieren (Regression-Check)
```

**Wichtig:** Das Skript analysiert standardmäßig **nur die Züge der Zielseite**
(Default `--player Martuni`, Substring-Match auf den White-/Black-Header,
case-insensitive). Wir wollen aus den eigenen Fehlern lernen, nicht aus denen
des Gegners — Stockfish-Zeit ist teuer, und ein Report voller Gegner-Patzer
verwässert die Cluster. Partien ohne Martuni-Header werden übersprungen und
auf stderr gemeldet. Für Self-Play-Partien (Martuni auf beiden Seiten) werden
automatisch beide Farben analysiert.

Der Bruch zwischen Schritt 3 und 4 ist Absicht: **das Skript schlägt keine
Code-Änderungen automatisch vor**. Es liefert Evidenz, Tobias entscheidet.
(Siehe CLAUDE.md → "Eigenleistung".)

## Das Skript — Design-Entscheidungen

Ablage: `tools/analyze_blunders.py`. Abhängigkeiten: `python-chess`, ein
lokaler Stockfish. Konkrete Entscheidungen mit Begründung:

### Phasen-Erkennung (`detect_phase`)

```python
if full_move <= 12 and npm >= 5500: "opening"
if npm <= 2000:                      "endgame"
else:                                 "middlegame"
```

- `npm` = Non-Pawn-Material beider Seiten in Centipawns.
- 5500 cp ≈ ~70 % Grundmaterial → wir sind noch in der Eröffnungsphase.
- 2000 cp ≈ höchstens noch zwei Leichtfiguren / ein Turm-Endspiel mit Rest.
- Die Zugnummer-Schranke verhindert, dass ein früher Damentausch in Zug 8
  fälschlich als "Endspiel" gezählt wird.

Alternative wären Martunis eigene `PhaseWeight`-Werte aus `pst.rs` — bewusst
nicht genutzt, damit das Skript **unabhängig von der Engine** kalibriert.
Sonst würde man die eigene Fehleinschätzung benutzen, um die eigene
Fehleinschätzung zu analysieren.

### Centipawn-Umrechnung (`score_to_cp`)

Matte werden auf ±100000 geclamped, kürzere Matte absolut größer. Damit lässt
sich `eval_before - eval_after` auch dann als Verlust messen, wenn eine Seite
von "Matt in 3" auf "Matt in 7" abrutscht oder ein Matt komplett verpasst.
Ohne Clamping würde `PovScore.score()` für Matte `None` liefern und die
Loss-Rechnung fällt aus.

### Blunder-Schwelle

Default 150 cp. Begründung:
- < 100 cp ist Rauschen bei 0.3 s Rechenzeit.
- 150 cp filtert die echten Patzer, ohne jede kleine Ungenauigkeit zu zählen.
- Für Endspiele ist 100 evtl. sinnvoller (kleinere Eval-Differenzen sind dort
  entscheidend) — `--threshold 100` per CLI.

### Motiv-Klassifikation (`classify_motifs`)

Heuristisch, bewusst nicht "clever". Ein Zug kann mehrere Motive bekommen:

| Motiv                 | Erkennung                                                                 | Mapping auf Martuni-Code                                  |
|-----------------------|---------------------------------------------------------------------------|-----------------------------------------------------------|
| `missed_mate`         | Stockfish hatte Matt für uns, wir haben es nicht gespielt                 | `search.rs`: Move Ordering, Mate-Distance-Pruning         |
| `allows_mate`         | Nach dem Zug hat der Gegner Matt                                          | `search.rs`: Quiescence, `eval.rs`: King-Safety-Gewicht   |
| `hangs_<piece>`       | SEE-lite: Figur ist nach dem Zug unterverteidigt; bei gleicher Zahl nur wenn der billigste Angreifer Material gewinnt | `search.rs`: Quiescence-Abdeckung, SEE in Move Ordering   |
| `missed_capture`      | Best Move war Schlagzug, gespielter Zug nicht                             | Move Ordering (MVV/LVA?), Quiescence-Tiefe                |
| `king_safety`         | Nach dem Zug greifen mehr gegnerische Figuren die 3×3-King-Zone an und es sind insgesamt mindestens 4 | `eval.rs`: `KING_SAFETY_*` Gewichte, Pawn-Shield          |
| `positional_collapse` | Fallback: Verlust ≥ 300 cp, aber kein taktisches Motiv gefunden           | `eval.rs`: fehlendes positionelles Wissen (s. unten)      |

**Wichtig:** der `hangs_*`-Check bricht beim ersten gefundenen Stück ab. Wenn
du feineres Mapping willst (Turm vs. Springer sind unterschiedliche
Diagnosen), das `break` rausnehmen.

#### Wartungshinweis: Präzisierung vom 2026-04-21

Die erste Fassung des Skripts war in zwei Punkten zu grob und hat Cluster
verzerrt:

- `king_safety` zählte effektiv **angegriffene Felder** in der King-Zone, nicht
  **verschiedene Angreifer**. Das konnte ruhige Stellungen mit vielen
  überdeckten Feldern fälschlich wie einen Angriff aussehen lassen.
- `hangs_<piece>` taggte eine Figur schon dann als "hängend", wenn der
  billigste Angreifer billiger war als das Opfer. Das erzeugte False Positives
  in Stellungen, in denen genug Verteidiger da waren.

Deshalb wurde die Heuristik nachgeschärft:

- `king_safety` zählt jetzt **distinct attacker** vor und nach dem Zug. Das
  Motiv feuert nur, wenn der eigene Zug die Zahl der gegnerischen Angreifer auf
  die King-Zone **erhöht** und danach mindestens **vier** gegnerische Figuren
  Druck machen. Ziel: echte Verschlechterungen markieren, nicht statisch
  ohnehin gefährliche Stellungen.
- `hangs_<piece>` benutzt jetzt eine bewusst einfache SEE-lite-Regel:
  `#attackers > #defenders` ist sofort verdächtig; bei gleicher Zahl wird nur
  dann markiert, wenn der billigste Angreifer Material gegen das Opfer gewinnt
  und der billigste Verteidiger keine gleich günstige Recapture-Struktur
  anbietet. Das ist immer noch heuristisch, liegt aber deutlich näher an der
  Dokumentation "unterverteidigt" als die alte Ein-Bedingungs-Regel.

Die Heuristiken bleiben absichtlich billig. Dieses Skript soll Batch-Reports
für viele Partien erzeugen, kein vollständiges Taktikmodul nachbauen.

### Was die Heuristik *nicht* sieht

Bewusste Lücken, die ggf. später ergänzt werden können:

- **Rückständige Bauern / schwache Felder**: nicht erkannt. Diese wandern ins
  `positional_collapse`-Bucket.
- **Freibauern-Technik**: nicht erkannt. Endspiel-Cluster werden also
  unterschätzt → beim Lesen des Reports berücksichtigen.
- **Horizon-Effekt**: nur indirekt über "taktisches Motiv in ruhiger
  Stellung" sichtbar. Bessere Diagnose: gleiche Stellung Martuni vs.
  Stockfish mit identischer Tiefe vergleichen.
- **Zeitmanagement-Fehler**: Blitzpartien mit vielen Buchzügen oder Pre-Moves
  erzeugen Rauschen. Mit `--min-movetime 0.3` werden Züge unter 0,3 Sekunden
  übersprungen (erfordert `%clk`-Annotationen im PGN).

## Mapping auf konkrete Stellschrauben

Wenn der Report ein Muster zeigt, hier der Ansatzpunkt im Code:

### Eval-Gewichte (`eval.toml` / `eval_config.rs`)
- King Safety: `KING_SAFETY_*`, `ATTACKER_WEIGHTS`, `SAFETY_TABLE`
- Passed Pawn: noch nicht vorhanden → neues Eval-Feature
- Bishop Pair: noch nicht vorhanden → neues Eval-Feature
- PST-Werte: `pst.rs`, beachte Tapered-Eval-Interpolation

### Fehlendes Eval-Wissen (neue Features in `eval.rs`)
- Rückständige Bauern, schwache Felder
- Rook on 7th, doppelte Türme auf offener Linie
- Outposts für Springer

### Such-Seite (`search.rs`)
- Move Ordering: MVV/LVA, Killer Moves, History Heuristic
- Quiescence-Abdeckung: alle Captures? auch Checks?
- Zeitmanagement: `MoveOverhead`, Soft/Hard Time Limits

### Endspielmodul (`endgame.rs`)
- Signaturen, die noch fehlen: KBPK, KRPvKR, KQvKR, …
- Siehe `docs/endgame.md` für die bereits umgesetzten Phasen A/B/C.

## Nutzung

```bash
# python-chess ist kein Projekt-Dep — bei Bedarf in einer venv installieren
pip install python-chess
```

### Eingabe: einzelne PGN-Datei(en)

```bash
# einzelne Partie (Default: nur Martunis Züge, 0.3 s Stockfish-Zeit)
python tools/analyze_blunders.py game.pgn

# mehrere Dateien
python tools/analyze_blunders.py game1.pgn game2.pgn game3.pgn
```

### Eingabe: ganzes Verzeichnis

```bash
# alle PGN-Dateien im Verzeichnis
python tools/analyze_blunders.py --game-dir ../lichess-bot/game_records/

# nur Dateien, die nach einem bestimmten Zeitpunkt geschrieben wurden
python tools/analyze_blunders.py --game-dir ../lichess-bot/game_records/ \
    --since 2026-04-12
python tools/analyze_blunders.py --game-dir ../lichess-bot/game_records/ \
    --since 2026-04-12T16:38
```

### Filter

```bash
# nur Verlustpartien analysieren (spart Stockfish-Zeit)
python tools/analyze_blunders.py --game-dir ../lichess-bot/game_records/ \
    --losses-only

# Buchzüge / Pre-Moves ignorieren (unter 0,3 s gespielt)
# — sinnvoll bei Blitzpartien mit vielen schnellen Anfangszügen
python tools/analyze_blunders.py --game-dir ../lichess-bot/game_records/ \
    --min-movetime 0.3

# anderen Spieler analysieren (z. B. für Vergleichszwecke)
python tools/analyze_blunders.py game.pgn --player Stockfish
```

### Stockfish-Einstellungen

```bash
# mehr Rechenzeit pro Zug → genauere Bewertung
python tools/analyze_blunders.py game.pgn --movetime 1.0

# feste Tiefe statt movetime (reproduzierbarer für Regression-Checks)
python tools/analyze_blunders.py game.pgn --depth 18

# mehr RAM und Threads (für schnelle Batch-Läufe)
python tools/analyze_blunders.py --game-dir ../lichess-bot/game_records/ \
    --threads 4 --hash 512

# anderen Stockfish-Pfad
python tools/analyze_blunders.py game.pgn --engine /usr/local/bin/stockfish
```

### Blunder-Schwelle

```bash
# strengere Schwelle (Standard: 150 cp)
python tools/analyze_blunders.py game.pgn --threshold 100

# lockerer, zeigt auch kleinere Ungenauigkeiten
python tools/analyze_blunders.py game.pgn --threshold 75
```

### Vollständiges Beispiel (typischer SEE-Feintuning-Lauf)

```bash
python tools/analyze_blunders.py \
    --game-dir ../lichess-bot/game_records/ \
    --since 2026-04-12T16:38 \
    --losses-only \
    --min-movetime 0.3 \
    --threshold 150 \
    --movetime 0.5 \
    --threads 2 --hash 256
```

### Alle Optionen auf einen Blick

| Option | Standard | Bedeutung |
|---|---|---|
| `pgn [...]` | — | Eine oder mehrere PGN-Dateien |
| `--game-dir DIR` | — | Verzeichnis mit PGN-Dateien (alle `*.pgn`) |
| `--since YYYY-MM-DD[THH:MM]` | — | Nur Dateien ab diesem UTC-Datum (mtime) |
| `--losses-only` | aus | Nur Partien analysieren, die Martuni verloren hat |
| `--player NAME` | `Martuni` | Welche Seite analysiert wird (Substring-Match) |
| `--min-movetime SECS` | `0.0` | Züge unter SECS Sekunden überspringen (`%clk` erforderlich) |
| `--threshold CP` | `150` | Centipawn-Verlust ab dem ein Zug als Patzer gilt |
| `--movetime SECS` | `0.3` | Stockfish-Analysezeit pro Zug |
| `--depth N` | — | Feste Suchtiefe statt movetime |
| `--engine PATH` | `stockfish` | Pfad zum Stockfish-Binary |
| `--threads N` | `1` | Stockfish-Threads |
| `--hash MB` | `128` | Stockfish-Hashtable-Größe in MB |

Der Report wird auf stdout geschrieben: erst die Summentabelle
(Phase / Motiv / Phase × Motiv), dann die Einzel-Blunder mit FEN, bestem Zug
laut Stockfish und cp-Loss. Für Regression-Tracking sinnvoll: stdout in eine
Datei umleiten und mit `diff` gegen den Lauf vor der Eval-Änderung vergleichen.

#### Wartungshinweis: Output-Vertrag vom 2026-04-21

Der Report-Output wurde an dieser Stelle bewusst nachgeschärft:

- Die Detailzeilen enthalten jetzt wieder explizit die **FEN vor dem Patzer**.
  Das macht einzelne Blunder direkt reproduzierbar und erlaubt, einen Report
  später ohne PGN-Suche auf konkrete Teststellungen zurückzuführen.
- Zusammenfassende Skip-Meldungen (`kein Header-Match`, `--losses-only`) gehen
  jetzt auf **stderr** statt auf stdout. Grund: stdout soll diffbar bleiben.
  Wer zwei Analyzer-Läufe vor und nach einer Eval-Änderung vergleicht, will nur
  den eigentlichen Report vergleichen und keine Laufmetadaten.

Praktisch heißt das:

```bash
python tools/analyze_blunders.py ... > report.txt 2> meta.log
```

`report.txt` ist dann stabiler für Regression-Vergleiche, `meta.log` enthält
die Laufumgebung und Überspring-Gründe.

## Für Claude in einer zukünftigen Session

Wenn Tobias sagt *"analysiere meine letzten Verlustpartien"* oder *"was macht
Martuni im Endspiel falsch"*:

1. PGNs einsammeln (Lichess-Export oder `berlinschach/games/`).
2. `tools/analyze_blunders.py` laufen lassen.
3. **Den Report lesen, nicht automatisch patchen.** Dieses Skript liefert
   Evidenz, nicht Entscheidungen.
4. Konkrete Vorschläge in `eval.toml` / `eval.rs` / `search.rs` formulieren
   und Tobias entscheiden lassen — das ist CLAUDE.md-Grundsatz
   "Eigenleistung": Optionen zeigen, nicht Code setzen.
5. Nach jeder Änderung: neue Testpartien spielen und den Report neu
   berechnen, um Regressionen zu sehen.
