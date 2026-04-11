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
| `hangs_<piece>`       | Mini-SEE: Figur nach dem Zug niedriger verteidigt als angegriffen         | `search.rs`: Quiescence-Abdeckung, SEE in Move Ordering   |
| `missed_capture`      | Best Move war Schlagzug, gespielter Zug nicht                             | Move Ordering (MVV/LVA?), Quiescence-Tiefe                |
| `king_safety`         | ≥ 4 Angreifer in der 3×3-Zone um den eigenen König                        | `eval.rs`: `KING_SAFETY_*` Gewichte, Pawn-Shield          |
| `positional_collapse` | Fallback: Verlust ≥ 300 cp, aber kein taktisches Motiv gefunden           | `eval.rs`: fehlendes positionelles Wissen (s. unten)      |

**Wichtig:** der `hangs_*`-Check bricht beim ersten gefundenen Stück ab. Wenn
du feineres Mapping willst (Turm vs. Springer sind unterschiedliche
Diagnosen), das `break` rausnehmen.

### Was die Heuristik *nicht* sieht

Bewusste Lücken, die ggf. später ergänzt werden können:

- **Rückständige Bauern / schwache Felder**: nicht erkannt. Diese wandern ins
  `positional_collapse`-Bucket.
- **Freibauern-Technik**: nicht erkannt. Endspiel-Cluster werden also
  unterschätzt → beim Lesen des Reports berücksichtigen.
- **Horizon-Effekt**: nur indirekt über "taktisches Motiv in ruhiger
  Stellung" sichtbar. Bessere Diagnose: gleiche Stellung Martuni vs.
  Stockfish mit identischer Tiefe vergleichen.
- **Zeitmanagement-Fehler**: das Skript kennt die Uhr nicht. Dafür muss man
  die PGN-`%clk`-Kommentare separat auswerten.

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

# einzelne Partie (Default: nur Martunis Züge)
python tools/analyze_blunders.py game.pgn --engine stockfish --movetime 0.3

# Batch mit strengerer Schwelle und mehr Rechenzeit
python tools/analyze_blunders.py games/*.pgn \
    --threshold 100 --movetime 1.0 --threads 4 --hash 512

# feste Tiefe statt movetime (reproduzierbarer für Regression-Checks)
python tools/analyze_blunders.py game.pgn --depth 18

# Anderen Spieler analysieren (z. B. für Vergleichszwecke)
python tools/analyze_blunders.py game.pgn --player Stockfish
```

Der Report wird auf stdout geschrieben: erst die Summentabelle
(Phase / Motiv / Phase × Motiv), dann die Einzel-Blunder mit FEN, bestem Zug
laut Stockfish und cp-Loss. Für Regression-Tracking sinnvoll: stdout in eine
Datei umleiten und mit `diff` gegen den Lauf vor der Eval-Änderung vergleichen.

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
