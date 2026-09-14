# Martuni — UCI-Schachengine in Rust

**Engine-Name:** `Martuni` | **Autor:** `Tobias Brendler`
Diese Werte sind fix und dürfen nicht geändert werden.

## Grundsatz: Eigenleistung

Die Engine-Logik (Suche, Stellungsbewertung, Strategie) wird von Tobias selbst entwickelt.
Externe Quellen dienen als Inspiration, aber Code darf **nie ungefragt kopiert oder eingebunden** werden.
Immer erklären, Optionen aufzeigen, Tobias entscheiden lassen.

Ausnahme: Infrastruktur (Board-Repräsentation, Zuggenerierung, UCI-Protokoll) darf auf Crates/Libraries aufbauen.

## Architektur

- **`chess`-Crate** (jordanbray, MIT) für Brettrepräsentation und legale Zuggenerierung — bewusste Entscheidung

## Aktueller Stand

Alle ursprünglichen Phase-1/2-Ziele sind umgesetzt:

- **UCI:** vollständig, inkl. `go ponder` / `ponderhit` mit echter Ponder-Suche (offene Deadline, TT-basierter Pondermove)
- **Varianten:** Standard, Chess960 (`UCI_Chess960`), Atomic, Crazyhouse, Antichess, King of the Hill, Horde, Three-Check und Racing Kings (`UCI_Variant atomic|crazyhouse|antichess|kingofthehill|horde|3check|racingkings`; Aliasse `giveaway`/`suicide`/`threecheck`); Varianten-Backends auf `shakmaty` (`BoardAtomic`, `BoardCrazyhouse`, generisches `BoardShak<P>`), Varianten-Eval-Hooks in `src/variants/`, Standardpfad weiter bit-exakt auf `chess`
- **Suche:** Alpha-Beta mit iterativem Deepening, PVS (Null-Window-Scout), Null-Move Pruning (R=2, min-depth 3, mit Zugzwang-Schutz), Late Move Reductions (Variante A: R=1 ab depth≥3 & Index≥3, R=2 ab depth≥6 & Index≥6; nur Non-PV, keine Captures/Promotions/Checks/Killer), Reverse Futility Pruning (depth ≤ 3), Quiescence Search, Transposition Table, korrekte Repetition-Detection (Stockfish-Stil: 1-fold in Spielhistorie ≠ Remis)
- **Move-Ordering:** TT-Move → SEE-basierte Captures (MVV/LVA) → Killer Moves → Countermove → Quiet Moves nach History-Heuristic; SEE-Pruning in Hauptsuche und Quiescence
- **Evaluation:** Material + Piece-Square-Tables (Tapered Midgame/Endgame), King Safety (3×3-Zone, Angreifer-Gewichte, SafetyTable, Pawn Shield), Endspiel-Heuristiken
- **Eröffnung:** Polyglot-Books (`.bin`) mit konfigurierbarer Prioritätsreihenfolge via `BOOK_FILES`, auch im Ponder-Modus aktiv
- **Konfiguration:** `.env` mit kaskadierter Suche; UCI-Optionen `Hash`, `MoveOverhead`, `Ponder` funktional wirksam

## Roadmap

Nächste Schritte, offene Themen und Verlauf der bisherigen Maßnahmen
stehen in [docs/roadmap.md](docs/roadmap.md).

## Lichess-Anbindung

Martuni spielt auf Lichess als **BOT Martuni** via `~/lichess-bot/` (Upstream: `lichess-bot-devs/lichess-bot`). Die Martuni-spezifische Config und Skripte liegen im Unterordner `lichess-bot/` dieses Repos (Token ist maskiert, das Original liegt unter `~/lichess-bot/config.yml`).

### Systemd-Service

Der Bot läuft als **`lichess-bot.service`** — nicht manuell starten, sonst entstehen doppelte Lichess-Sessions!

```
# Pfade sind serverspezifisch
Unit:             /etc/systemd/system/lichess-bot.service
User:             <systemuser>
WorkingDirectory: <homedir>/lichess-bot
ExecStart:        venv/bin/python lichess-bot.py
Restart:          always
```

- **Hard Restart** (unterbricht laufende Partien): `sudo systemctl restart lichess-bot.service`
- **Graceful:** `quit_after_all_games_finish: true` in config.yml setzen, warten bis keine Partie läuft, dann restart.
- **Logs:** `journalctl -u lichess-bot.service -f`
- Config-Änderungen und Engine-Rebuilds (`cargo build --release`) werden erst nach Restart wirksam.

### Challenge-Cron

`challenge_cron.py` läuft stündlich (Crontab, `45 * * * *`) und fordert automatisch einen Online-Bot heraus (abwechselnd 5+0 Blitz und 15+10 Rapid). Ergebnisse werden in `challenge_cron_tracking.json` erfasst. Log: `lichess_bot_auto_logs/challenge_cron.log`.

### Blunder-Analyse (seit 14.09.2026 ausgelagert auf den Grok-Bot)

Dieser Server rechnet **keine Stockfish-Analysen mehr selbst** (Server am
Limit). `tools/analyze_cron.py` ist in der Crontab auskommentiert. Stattdessen
läuft `tools/schleuse_sync.py` alle 10 Minuten und tauscht Dateien mit dem
Grok-Bot über `~/grok_bot_schleuse/` aus — Protokoll, Betrieb und Rückweg in
[docs/grok-schleuse.md](docs/grok-schleuse.md).

- **Schleuse:** `outbox/<pgn>.pgn` (offene Partien, max. 50) → Grok-Bot
  analysiert mit `analyze_blunders.py` (Stockfish bzw. Fairy-Stockfish,
  Tiefe 17) → `inbox/<pgn>.json` bzw. `inbox/<pgn>.failed.json` → Merge in
  `analyse-<datum>.json` / `analyse-<datum>-varianten.json`, Datei nach `done/`.
- **Konfiguration:** weiterhin `tools/analyze_cron.config.json` (lokal,
  `skip-worktree` + gitignored). Nach einer Archivierung müssen `output` und
  `variant-output` auf die neuen Zieldateien zeigen. **Vor dem Speichern JSON
  validieren** — eine kaputte Config legt den Sync still (Lehre vom 06.09.2026).
  Neue optionale Schlüssel: `schleuse-dir`, `max-outbox`.
- **Quarantäne:** `tools/analyze_cron.quarantine.json` wie bisher; ein
  `.failed.json` vom Grok-Bot zählt als Fehlschlag, ab `max-failures` (3)
  wird die PGN übersprungen.
- **Log:** `logs/schleuse_sync.log` (`grep -E "FAILED|ERROR|QUARANTINE"`).
  `info` zeigt die Warteschlange (`outbox/inbox`) und den Grok-Heartbeat.

```bash
cargo build --release
echo -e "uci\nisready\nposition startpos\ngo movetime 1000\nquit" | ./target/release/martuni
```
