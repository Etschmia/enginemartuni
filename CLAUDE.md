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
- Module:
  - `uci.rs` — UCI-Protokoll inkl. Ponder-Handling
  - `position.rs` — Board-Wrapper
  - `search.rs` — Alpha-Beta mit iterativem Deepening, Quiescence, TT-Integration
  - `eval.rs` — Stellungsbewertung (Material, PST, King Safety, Pawn Shield, Endspiel-Heuristiken)
  - `pst.rs` — Piece-Square-Tables mit Tapered Eval (Midgame/Endgame-Interpolation)
  - `endgame.rs` — spezialisierte Endspiel-Bewertung
  - `eval_config.rs` — laufzeit-konfigurierbare Eval-Parameter (analog zu `.env`)
  - `tt.rs` — Transposition Table (Zobrist-basiert)
  - `polyglot/` — Polyglot-Buch-Reader (`book.rs`, `hash.rs`, `random.rs`)
  - `config.rs` — `.env`-Loader mit kaskadierter Suche (CWD → Binary-Dir → Projekt-Root)
  - `options.rs` — UCI-Optionen

## Aktueller Stand

Alle ursprünglichen Phase-1/2-Ziele sind umgesetzt:

- **UCI:** vollständig, inkl. `go ponder` / `ponderhit` mit echter Ponder-Suche (offene Deadline, TT-basierter Pondermove)
- **Suche:** Alpha-Beta mit iterativem Deepening, Quiescence Search, Transposition Table
- **Evaluation:** Material + Piece-Square-Tables (Tapered Midgame/Endgame), King Safety (3×3-Zone, Angreifer-Gewichte, SafetyTable, Pawn Shield), Endspiel-Heuristiken
- **Eröffnung:** Polyglot-Books (`gm2001.bin`, `komodo.bin`, `rodent.bin`) mit Prioritätsreihenfolge, auch im Ponder-Modus aktiv
- **Konfiguration:** `.env` mit kaskadierter Suche; UCI-Optionen `Hash`, `MoveOverhead`, `Ponder` funktional wirksam

## Roadmap

- **Auswertung nach >100 Partien** — die am 26.04.2026 ausgerollten Änderungen
  (King-Exposure entschärft + phase-getapert, Check-Extension +2 → +1 Standard,
  MAX_EXTENSION_PER_LINE 6 → 4, neuer Endgame-Malus für eingemauerten Turm)
  in der Praxis prüfen. Auswertungspunkte: Endgame-Blunder/Partie (Ausgangswert
  0.49), `exposed_king`/Partie (0.16), `positional_collapse`/Partie (0.35),
  Eval-Pessimismus-Fälle. Wenn die Zahlen runtergehen, ist der Knoten gelöst.
- **Null-Move Pruning** — danach. Plan steht in der Memory.

## Testumgebung

Die Engine wird primär gegen `/home/librechat/berlinschach` getestet (UCI-Web-GUI).
Eintrag in `berlinschach/engines.json` ist vorhanden.

## Lichess-Anbindung

Martuni spielt auf Lichess als **BOT Martuni** via `/home/librechat/lichess-bot/` (Upstream: `lichess-bot-devs/lichess-bot`). Die Martuni-spezifische Config und Skripte liegen im Unterordner `lichess-bot/` dieses Repos (Token ist maskiert, das Original liegt unter `/home/librechat/lichess-bot/config.yml`).

### Systemd-Service

Der Bot läuft als **`lichess-bot.service`** — nicht manuell starten, sonst entstehen doppelte Lichess-Sessions!

```
Unit:             /etc/systemd/system/lichess-bot.service
User:             librechat
WorkingDirectory: /home/librechat/lichess-bot
ExecStart:        venv/bin/python lichess-bot.py
Restart:          always
```

- **Hard Restart** (unterbricht laufende Partien): `sudo systemctl restart lichess-bot.service`
- **Graceful:** `quit_after_all_games_finish: true` in config.yml setzen, warten bis keine Partie läuft, dann restart.
- **Logs:** `journalctl -u lichess-bot.service -f`
- Config-Änderungen und Engine-Rebuilds (`cargo build --release`) werden erst nach Restart wirksam.

### Challenge-Cron

`challenge_cron.py` läuft stündlich (Crontab, `45 * * * *`) und fordert automatisch einen Online-Bot heraus (abwechselnd 5+0 Blitz und 15+10 Rapid). Ergebnisse werden in `challenge_cron_tracking.json` erfasst. Log: `lichess_bot_auto_logs/challenge_cron.log`.

```bash
cargo build --release
echo -e "uci\nisready\nposition startpos\ngo movetime 1000\nquit" | ./target/release/martuni
```
