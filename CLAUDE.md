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

## Lichess-Anbindung (seit 18.11.2026 wieder auf diesem Server)

Martuni spielt auf Lichess als **BOT Martuni** von **diesem Server**
(martuni.de). Vom 13.10. bis 18.11.2026 lief der Bot auf Horst (SYR-PE-BUTDEV,
Claude-Session „Horst“, per `SendMessage` erreichbar); dessen Unit
`lichess-bot-martuni.service` ist gestoppt und disabled — **dort nie wieder
starten** (doppelte Lichess-Session mit demselben Token). Horsts Checkout
`/var/www/but2/botdir/lichess-bot-martuni` liegt noch als Rückfall.

- **Bot:** Checkout `~/lichess-bot` (Fork `Etschmia/lichess-bot`, `config.yml`
  gitignored, enthält den Token), Unit `lichess-bot.service` (SIGINT,
  `KillMode=mixed`, `TimeoutStopSec=900`: laufende Partien werden zu Ende
  gespielt). Engine aus `~/enginemartuni/target/release`.
- **Rollout:** auf `master` committen/pushen, `cargo build --release`.
  Die **Build-mtime des Binaries ist der Rollout-Zeitpunkt** (Lookback, Cut
  für die Archivierung) — in der Roadmap festhalten. Kein Bot-Restart nötig
  (lichess-bot startet die Engine pro Partie neu).
- **`.env`** hier ist wieder die Live-Konfiguration (Buch, Hash, Syzygy);
  `config.yml` setzt zusätzlich `Hash`, `MoveOverhead`, `SyzygyPath`.
- **Challenges:** `accept_bot: false` — **nur noch Menschen** (Tobias-Entscheid
  18.11.2026, Bot-Challenges hatten überhandgenommen). concurrency 2,
  ausgehende Challenge-Crons bleiben aus.
- **Partien:** lichess-bot schreibt direkt nach `~/martuni_games/game_records/`,
  Archiv `~/martuni_games/game_archiv/`. Von dort laufen Schleuse, `info` und
  Auswertungen.
- **Upstream-Sync** des Forks: Cron Sa 06:27
  (`LICHESS_BOT_SERVICE=lichess-bot.service lichess-bot-upstream-sync.sh`,
  Log `~/lichess-bot/lichess_bot_auto_logs/upstream-sync.log`, Konfliktmail
  über `resend.env`). Betriebs-Doku: `~/grok_bot_schleuse/BETRIEB-lichess-bot.md`.
- **A/B-Matches** konkurrieren wieder mit Live-Partien um die 2 vCPU —
  lange Matches möglichst nachts.

### Blunder-Analyse (seit 14.09.2026 ausgelagert, Worker heute auf Horst)

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
- **Archivierung nach einem Rollout:** `tools/archive_after_rollout.py`
  (Probelauf ohne, scharf mit `--execute`) bzw. Claude-Skill `martuni-archiv`:
  Partien vor dem Cut nach `game_archiv/`, Analysedateien nach `archiv/`,
  Config auf `analyse-<Datum>.json` umstellen.
- **Quarantäne:** `tools/analyze_cron.quarantine.json` wie bisher; ein
  `.failed.json` vom Grok-Bot zählt als Fehlschlag, ab `max-failures` (3)
  wird die PGN übersprungen.
- **Log:** `logs/schleuse_sync.log` (`grep -E "FAILED|ERROR|QUARANTINE"`).
  `info` zeigt die Warteschlange (`outbox/inbox`) und den Grok-Heartbeat.

```bash
cargo build --release
echo -e "uci\nisready\nposition startpos\ngo movetime 1000\nquit" | ./target/release/martuni
```
