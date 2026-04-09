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
- **`rand`-Crate** für Zufallszüge (Phase 1)
- Module: `uci.rs` (Protokoll), `position.rs` (Board-Wrapper), `search.rs` (Zugsuche), `options.rs` (UCI-Optionen)

## Aktueller Stand

**Phase 1 ("Martuni 0")** — abgeschlossen:
- Vollständige UCI-Konformität (uci, isready, position, go, stop, quit, setoption)
- Zufällige legale Züge mit simulierter Bedenkzeit
- UCI-Optionen: Hash, MoveOverhead (noch ohne Funktion)

## Roadmap

- Alpha-Beta-Suche mit iterativem Deepening
- Stellungsbewertung: Material + Piece-Square-Tables + Königssicherheit
- Polyglot Opening Books (.bin)
- Ponder-Support (berlinschach sendet bereits `go ponder` / `ponderhit`)

## Testumgebung

Die Engine wird primär gegen `/home/librechat/berlinschach` getestet (UCI-Web-GUI).
Eintrag in `berlinschach/engines.json` ist vorhanden.

```bash
cargo build --release
echo -e "uci\nisready\nposition startpos\ngo movetime 1000\nquit" | ./target/release/martuni
```
