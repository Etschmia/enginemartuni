# Grok-Schleuse: Blunder-Analyse auf dem Grok-Bot

**Stand: 14.09.2026.** Der Martuni-Server ist am Limit (Lichess-Bot + Arbeit
am Server). Deshalb rechnet dieser Server **keine Stockfish-Analysen mehr
selbst**. Die Analyse-Partien werden über ein Austauschverzeichnis an den
Grok-Bot (anderer Rechner) übergeben, der dort `tools/analyze_blunders.py`
mit Stockfish bzw. Fairy-Stockfish ausführt und die Ergebnisse zurücklegt.

Der Rest der Kette bleibt, wie er war: dieselben Zustandsdateien
`analyse-<datum>.json` / `analyse-<datum>-varianten.json`, dieselbe Config
`tools/analyze_cron.config.json`, dieselbe Quarantäne, derselbe `info`-Befehl,
dieselbe Archivierung, derselbe Report (`analyze_blunders.py --report`).

## Überblick

```
Martuni-Server                      ~/grok_bot_schleuse/            Grok-Bot (remote)
──────────────────────────────      ───────────────────────────     ──────────────────────────
tools/schleuse_sync.py (Cron        outbox/<pgn>.pgn          ──►   analyze_blunders.py
alle 10 Min):                                                        --engine stockfish |
  • offene PGNs kopieren      ──►                                    fairy-stockfish
  • Ergebnisse mergen         ◄──   inbox/<pgn>.json          ◄──   --output <pgn>.json
  • Fehlschläge zählen        ◄──   inbox/<pgn>.failed.json   ◄──   (bei Fehler)
  • verarbeitet → done/             done/                            löscht outbox/<pgn>.pgn
                                    status.json               ◄──   Heartbeat (optional)
```

Kein Stockfish, kein Fairy-Stockfish und kein `nice`/`ionice` mehr auf dem
Martuni-Server. `schleuse_sync.py` kopiert und merged nur JSON.

## Was wo läuft

| Wo | Was | Wann |
|---|---|---|
| Martuni-Server, Crontab | `tools/schleuse_sync.py` | `*/10 * * * *` |
| Martuni-Server, Crontab | `tools/analyze_cron.py` | **abgeschaltet** (auskommentiert, 14.09.2026) |
| Grok-Bot | Poll auf `outbox/`, Analyse, Ergebnis nach `inbox/` | Intervall laut Abstimmung (Vorschlag: alle 5 Min, älteste PGN zuerst) |

Crontab-Backup vor der Umstellung: `logs/crontab.backup-2026-09-14.txt`.

## Verzeichnisprotokoll (`~/grok_bot_schleuse/`)

Eigentumsregel: **jede Seite schreibt nur in „ihre“ Dateien**, damit es keine
Wettläufe gibt.

| Pfad | Schreibt | Liest | Inhalt |
|---|---|---|---|
| `outbox/<pgn>.pgn` | Martuni-Server (anlegen), Grok-Bot (löschen nach Ergebnis) | Grok-Bot | Eine zu analysierende Partie, Dateiname identisch mit `game_records/` |
| `inbox/<pgn>.json` | Grok-Bot | Martuni-Server | Ergebnis: exakt die Datei, die `analyze_blunders.py --output` für **genau diese eine** PGN erzeugt |
| `inbox/<pgn>.failed.json` | Grok-Bot | Martuni-Server | `{"error": "...", "rc": <int oder null>, "engine": "...", "at": "<ISO-Zeit>"}` |
| `done/` | Martuni-Server | – | Archiv der verarbeiteten `inbox`-Dateien |
| `status.json` | Grok-Bot | `info` | Heartbeat: `{"last_run": "<ISO>", "engine": "...", "variant_engine": "...", "queue": <n>, "current": "<pgn>"}` (optional) |
| `PROTOKOLL.md` | Martuni-Server | Grok-Bot | Kopie dieses Protokolls |
| `analyze_blunders.py` | Martuni-Server | Grok-Bot | Referenzkopie des Analyse-Skripts (Stand `cde394b`) |

`<pgn>` ist immer der volle Dateiname inklusive `.pgn`, also z. B.
`inbox/Alarm2001 vs Martuni - ybPqosdz.pgn.json`. Leerzeichen im Namen sind
normal (Lichess-Export).

### Atomarität

Beide Seiten schreiben erst in eine `*.tmp`-Datei und benennen dann um
(`rename`), damit die Gegenseite nie eine halbe Datei sieht. Dateien mit
Endung `.tmp` werden von beiden Seiten ignoriert.

### Ablauf auf der Grok-Seite (Soll)

1. `outbox/*.pgn` auflisten, älteste zuerst (mtime); Dateien überspringen,
   für die schon `inbox/<pgn>.json` oder `inbox/<pgn>.failed.json` existiert.
2. `[Variant "..."]` im PGN-Header lesen. Standard, Chess960 / Fischerandom
   und „From Position“ (oder fehlender Header) → `stockfish`. Alles andere
   (Antichess, Atomic, Crazyhouse, King of the Hill, Horde, Three-check,
   Racing Kings) → `fairy-stockfish`.
3. Analyse mit **denselben Parametern wie bisher hier**, in eine frische
   Ausgabedatei (darf vorher nicht existieren, sonst merged das Skript):

   ```bash
   python3 analyze_blunders.py \
       --engine <stockfish|fairy-stockfish> \
       --depth 17 --threads 1 --hash 256 --min-movetime 0 \
       --output "<arbeitsverzeichnis>/<pgn>.json" \
       "outbox/<pgn>"
   ```

   `--threads` darf höher sein, wenn der Rechner Kerne übrig hat; `--depth 17`
   sollte gleich bleiben, sonst sind die Blunder/Partie-Kennzahlen nicht mehr
   mit den bisherigen Wochen vergleichbar.
4. Bei `rc == 0`: Ergebnisdatei atomar nach `inbox/<pgn>.json` legen, dann
   `outbox/<pgn>.pgn` löschen.
   Bei `rc != 0` oder Ausnahme: `inbox/<pgn>.failed.json` schreiben, dann
   `outbox/<pgn>.pgn` löschen. Der Martuni-Server bietet die PGN beim nächsten
   Export erneut an; nach 3 Fehlschlägen wandert sie in Quarantäne.
5. Optional `status.json` aktualisieren.

Erwartete Laufzeit: ~10 Minuten pro Partie bei Tiefe 17 auf einem Kern
(Erfahrungswert von hier), Varianten mit Fairy-Stockfish ähnlich.

### Ablauf auf der Martuni-Seite (`tools/schleuse_sync.py`)

1. PID-Lock (`tools/schleuse_sync.pid`).
2. **Import:** jede `inbox/*.json` in die passende Zustandsdatei mergen.
   Die Zuordnung Standard vs. Variante trifft der Server selbst über den
   lokalen PGN-Header (wie der alte Cron); ist die PGN hier schon archiviert,
   gilt ein optionales Feld `"variant"` im Ergebnis, sonst Standard. Dedupe
   über `(game_id, ply)`. Danach Datei nach `done/`, Outbox-Kopie löschen,
   Quarantäne-Zähler zurücksetzen. `.failed.json` erhöht den Zähler.
3. **Export:** alle PGNs aus `game_records/`, die weder analysiert noch in
   Quarantäne noch bereits in `outbox/` oder `inbox/` sind, chronologisch nach
   `outbox/` kopieren, höchstens `max-outbox` (Default 50) gleichzeitig.
4. Zusammenfassung ins Log.

## Konfiguration

`tools/analyze_cron.config.json` (lokal, gitignored, skip-worktree) wird
unverändert weiterbenutzt. Relevante Schlüssel:

| Schlüssel | Bedeutung |
|---|---|
| `game-dir`, `output`, `variant-output`, `max-failures` | wie bisher |
| `schleuse-dir` | neu, optional; Default `~/grok_bot_schleuse` |
| `max-outbox` | neu, optional; Default 50 Dateien gleichzeitig in `outbox/` |
| `depth`, `threads`, `hash`, `min-movetime`, `engine`, `variant-engine` | vom Server nicht mehr benutzt, gelten als **Sollwerte für die Grok-Seite** |

**Archivierung** läuft wie gehabt: neue Zieldateien in `output` /
`variant-output` eintragen, JSON validieren. `schleuse_sync.py` liest die
Config bei jedem Lauf neu.

## Betrieb und Fehlersuche

```bash
# Log des Syncs (eine SUMMARY-Zeile pro Lauf)
tail -n 20 logs/schleuse_sync.log
grep -E "FAILED|ERROR|QUARANTINE" logs/schleuse_sync.log

# Manueller Lauf / Probelauf
.venv/bin/python3 tools/schleuse_sync.py --dry-run && tail -n 5 logs/schleuse_sync.log
.venv/bin/python3 tools/schleuse_sync.py

# Warteschlange auf einen Blick (auch in `info`)
ls ~/grok_bot_schleuse/outbox | wc -l; ls ~/grok_bot_schleuse/inbox
```

Typische Zustände:

- **`outbox` voll (50), `inbox` leer, `done` wächst nicht:** der Grok-Bot holt
  nicht ab. `status.json` prüfen, Tobias gibt dem Grok-Bot Bescheid.
- **Viele `.failed.json`:** Fehlertext steht in der Datei und im Log; meist
  Engine-/Pfadproblem auf der Grok-Seite. Quarantäne greift nach 3 Versuchen,
  damit nichts blockiert.
- **`inbox/<pgn>.json nicht lesbar`:** Datei gerade in Arbeit; nächster Tick.
- **Ergebnis für eine bereits archivierte PGN:** wird trotzdem gemerged
  (Zuordnung dann über `"variant"` im Ergebnis oder Standard).

## Rückweg

Falls die Auslagerung wieder zurückgedreht werden soll:

1. Crontab: die `schleuse_sync.py`-Zeile auskommentieren, die alte
   `analyze_cron.py`-Zeile wieder aktivieren (Backup siehe oben).
2. `outbox/` leeren (die PGNs liegen ja weiterhin in `game_records/`), Rest
   von `inbox/` einmal per `schleuse_sync.py` importieren.

Beide Skripte teilen sich Zustandsdateien, Config und Quarantäne, es gibt
keine Migration.
