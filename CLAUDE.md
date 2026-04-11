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

_Offen — wird bei Bedarf mit Tobias neu festgelegt._

## Testumgebung

Die Engine wird primär gegen `/home/librechat/berlinschach` getestet (UCI-Web-GUI).
Eintrag in `berlinschach/engines.json` ist vorhanden.

Lichess-Anbindung via `/home/librechat/lichess-bot/` (config.yml) — in Einrichtung.

```bash
cargo build --release
echo -e "uci\nisready\nposition startpos\ngo movetime 1000\nquit" | ./target/release/martuni
```
