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
- **Eröffnung:** Polyglot-Books (`.bin`) mit konfigurierbarer Prioritätsreihenfolge via `BOOK_FILES`, auch im Ponder-Modus aktiv
- **Konfiguration:** `.env` mit kaskadierter Suche; UCI-Optionen `Hash`, `MoveOverhead`, `Ponder` funktional wirksam

## Roadmap

- **Auswertung 01.05.2026 (162 Partien) — DONE.** Die 28.04-Anpassung hat
  geliefert: Endgame-Blunder/Partie 0.60 → 0.358, exposed_king 0.14 → 0.086,
  positional_collapse 0.24 → 0.160 (alle deutlich besser). Einziger
  Negativtrend: `missed_mate`/Partie 0.075 → 0.105. Inspektion der 17 Fälle
  zeigt: nahezu alle sind Stellungen, in denen Martuni schon klar gewann
  (`martuni=+6cp .. +21cp`) und nur das schnellste Matt nicht fand —
  strukturelles Tiefen-Problem, kein Eval-Fehler. Lichess-Rating: Blitz
  1864 → 1921, Rapid 1928 → 1975 (3 Tage). Befund hat NMP-Implementierung
  ausgelöst.
- **Eingemauerter Turm (`rook_trapped_endgame_penalty`) — DONE.** Am 28.04.
  mit `tools/diagnose_rook_trapped.py` geprüft: Term feuert in nur 2 von 104
  Endgame-Blundern (1.9 %), in beiden Fällen sachlich korrekt. Nicht der
  Treiber der Endgame-Verschlechterung — bleibt bei `-10`, kein Anlass zum
  Justieren.
- **Null-Move Pruning + PVS — DONE 01.05.2026.** Plan stand in
  `docs/null-move-pruning.md` (NMP-Konzept), PVS wurde gleich mitgeliefert,
  weil NMP ohne Nullfenster-Knoten in der Suche nie greift. Verifikation:
  Mittelspiel-Stellung −43 % Knoten auf gleicher Tiefe; `missed_mate`-
  Stellung aus dem Analyse-File (Martuni vs Bot5551, Zug 29) wird jetzt mit
  `mate 6` gelöst statt vorher unentdeckt zu bleiben. R = 2 konstant,
  Mindesttiefe 3, Zugzwang-Schutz via `has_non_pawn_material`.
- **Nächste Auswertung nach >100 Partien.** Primärer Ziel-Indikator:
  `missed_mate`/Partie soll von 0.105 deutlich runter (Erwartung Richtung
  0.04 wieder, weil das genau der NMP-Effekt ist). Sekundär: Gesamt-Blunder
  soll fallen, Rating-Erwartung +50–80 Elo. Wenn alles gut: weiter mit
  LMR (Late Move Reductions, siehe `project_lmr_plan` Memory) oder
  NMP-Verfeinerungen (adaptive R, Verification Search).

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

```bash
cargo build --release
echo -e "uci\nisready\nposition startpos\ngo movetime 1000\nquit" | ./target/release/martuni
```
