# Syzygy-Tablebases in Rust — Integrationsoptionen

Notiz / Idee (Stand 2026-05-16). Recherche-Ergebnis aus einem Blick darauf,
wie andere Engines Syzygy-Endspieldatenbanken einbinden — als Referenz, falls
wir Tablebase-Probing in dieser Engine ergänzen wollen.

Hintergrund: Syzygy ist der De-facto-Standard für Endspieldatenbanken in
modernen Engines. Die Tabellen (`.rtbw` für Win/Draw/Loss, `.rtbz` für
Distance-to-Zero) sind frei verfügbar; nur das Probing muss in die Engine.
Die C-Referenzimplementierung ist Ronald de Mans **Fathom** (bzw. dessen
gepflegter Fork **Pyrrhic**). Für Rust gibt es zwei etablierte Wege.

## Option 1 — `shakmaty-syzygy` (pure Rust)

- Repo: <https://github.com/niklasf/shakmaty-syzygy>
- Crate: <https://crates.io/crates/shakmaty-syzygy>
- Docs: <https://docs.rs/shakmaty-syzygy>
- Autor: Niklas Fiekas (auch hinter `shakmaty` und Teilen von lichess.org)

Reine Rust-Reimplementierung des Probing-Codes — kein C, kein FFI. Baut auf
der `shakmaty`-Bibliothek auf (eigenes Board-Modell, BBC-ähnliche
Bitboards). Unterstützt WDL- und DTZ-Probing für bis zu 7 Steine.

**Stärken**
- Keine `unsafe`-Blöcke, kein C-Buildschritt
- Saubere, idiomatische Rust-API
- Gut für Analyse-Tools, Endgame-Explorer, Offline-Auswertung

**Schwächen / Caveats**
- Etwas allokationslastiger als Fathom/Pyrrhic — eher nicht primär für
  hochfrequentes In-Search-Probing optimiert
- Wenn unsere Board-Repräsentation nicht `shakmaty` ist, muss zwischen den
  Datenstrukturen konvertiert werden

## Option 2 — `pyrrhic-rs` (FFI-Wrapper um Pyrrhic/Fathom)

- Repo: <https://github.com/Algorhythm-sxv/pyrrhic-rs>
- Crate: <https://crates.io/crates/pyrrhic-rs>

Safe Rust-Wrapper um **Pyrrhic** (gepflegter Fork von Fathom, dem
C-Standardcode, den auch Stockfish-Derivate, Ethereal etc. einbinden).
Speziell auf den In-Search-Probing-Use-Case in Engines zugeschnitten.

**Stärken**
- Performance-Charakteristik praktisch identisch zu Stockfish & Co.
- Thread-safe Wrapper schirmt die `unsafe` C-API ab
- 1:1-äquivalent zu dem Weg, den der ChessBot-Lichess-Bot bereits in C
  verwendet (Fathom direkt) — gleiche Tabellenformate, gleiche Semantik
- Reife Codebasis mit Engine-Verifikation in der Praxis

**Schwächen / Caveats**
- C-Build-Dependency (C-Compiler im Build-Pfad nötig)
- `unsafe` ist nur gekapselt, nicht eliminiert
- API ist näher an der C-Welt → weniger "rustig"

## Empfehlung (falls wir es jemals einbauen)

- **In-Search-Probing in der Engine** → `pyrrhic-rs`. Es ist der natürliche
  Pfad: bewährte Performance, gleiches Format wie Fathom, geringer
  Abstraktions-Overhead. Root-Probing + WDL-Probing in der Suche lassen
  sich mit überschaubarem Aufwand verdrahten.
- **Reines Analyse-/Tooling-Probing** (Endgame-Browser, Offline-Trainer,
  Stellungsanalyse außerhalb der Suche) → `shakmaty-syzygy`. Hier zählen
  saubere API und Wartbarkeit mehr als Probing-Latenz.

## Tabellen selbst

Beide Crates probieren nur — die `.rtbw`/`.rtbz`-Dateien müssen separat
heruntergeladen werden (z. B. tablebase.lichess.ovh, syzygy-tables.info).
Standardumfang in Engines ist 3-, 4- und 5-Steiner (~1 GB); 6-Steiner sind
ein Vielfaches größer, 7-Steiner mehrere Terabyte. Tabellen gehören nie ins
Repo, sondern werden zur Laufzeit per Konfigurationspfad eingebunden.
