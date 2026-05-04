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
- **Suche:** Alpha-Beta mit iterativem Deepening, PVS (Null-Window-Scout), Null-Move Pruning (R=2, min-depth 3, mit Zugzwang-Schutz), Late Move Reductions (Variante A: R=1 ab depth≥3 & Index≥3, R=2 ab depth≥6 & Index≥6; nur Non-PV, keine Captures/Promotions/Checks/Killer), Quiescence Search, Transposition Table, korrekte Repetition-Detection (Stockfish-Stil: 1-fold in Spielhistorie ≠ Remis)
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
- **Auswertung 04.05.2026 (175 Partien) — DONE.** NMP-Effekt bestätigt:
  `missed_mate`/Partie 0.105 → **0.057** (−46 %), Lichess Blitz 1921 → 1965
  (+44), Rapid 1975 → 2016 (+41). Endgame-Rate stabil (0.358 → 0.337),
  keine Zugzwang-Regression. Neuer Hotspot `allows_mate` 0.126/P. (22 Fälle),
  primär Tiefenproblem in bereits verlorenen Stellungen. Details:
  `project_auswertung_2026_05_04` Memory, Datei `analyse_04.05.2026.txt`.
- **LMR implementiert (04.05.2026).** Variante A nach Tobias-Spezifikation:
  Stufenformel (R=1 ab depth≥3 & Index≥3, R=2 ab depth≥6 & Index≥6),
  nur Non-PV-Knoten, ab dem 4. sortierten Zug. Ausgeschlossen von Reduktion:
  Captures (über `sm.see_val`), Promotionen, Schachgebote, Züge im Schach,
  Killer-Moves, alle Züge mit aktiver Extension. Re-Search-Kaskade:
  reduzierte Nullfenster-Suche → bei Fail-High volle Tiefe Nullfenster →
  bei `alpha < score < beta` PVS-Re-Search mit vollem Fenster.
  Verifikation: `missed_mate`-Stellung Tiefe 9/mate 6 in 2.7 s mit 6.8 M
  Knoten (vorher Tiefe 7/mate 6 in 5.7 s mit 17.4 M Knoten). Wichtig beim
  Implementieren war: `.max(1)` auf `scout_depth` darf nur greifen, wenn
  tatsächlich reduziert wird, sonst wird der natürliche Übergang
  `new_depth==0` → Quiescence aufgebläht und die Suche kollabiert.
  Konzeption: `docs/lmr-plan.md`. History-Heuristic bewusst NICHT als
  zusätzliches LMR-Kriterium — wirkt nur über die Zugreihenfolge.
- **Nächste Auswertung nach ≥100 Partien LMR.** Primäre Ziel-Indikatoren:
  `allows_mate`/Partie 0.126 → ~0.07, `missed_mate` weiter Richtung 0.03,
  Rating +30–60 Elo. Wenn positiv und keine neuen Regressionen
  (`missed_capture`, `exposed_king`): dynamische Figurenwerte angehen
  (`docs/vorbereiteter_Prompt_dynamische_Figurenbewertung.md`,
  schrittweiser Rollout in 3 Phasen). NMP-Verfeinerungen (adaptive R,
  Verification Search) erst, wenn die Endgame-Rate Anlass gibt — aktuell
  kein Druck.
- **Repetition-Detection korrigiert (02.05.2026).** `state.history.contains`
  zählte vorher 1-fold in Spielhistorie als Remis und blockierte ruhige
  Best-Moves (Repro: vGwmaXUy, 19.Ng5?? statt 19.Qe4). Neuer Helfer
  `is_repetition_draw` trennt Spielhistorie und Suchpfad
  (`SearchState.root_history_len`); Unit-Tests in `search::tests`.
- **Analyse-Skript verbessern — DONE 02.05.2026.** Pauschales
  `--min-movetime 0.3` ersetzt durch per-Klasse-Schwellen über
  `est = base + 40·inc` (Lichess-Konvention): bullet 0s, blitz 1s, rapid 3s,
  classical 5s. Schwellen sind `int` (PGN-Clocks haben Sekunden-Auflösung),
  `--min-movetime` (float, Default 0.3) bleibt als Fallback für fehlende /
  unparsbare `[TimeControl]`-Tags. Skip-Meldung auf stderr zeigt jetzt Klasse
  + Schwelle. Doku: `docs/blunder-analyse.md` (Wartungshinweis
  „Movetime-Filter pro Zeitkontrolle (2026-05-02)"); ursprüngliche Planung
  in `docs/tool-änderung.md` als umgesetzt markiert.

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
