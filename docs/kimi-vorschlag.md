# Möglichkeiten zur Steigerung der Spielstärke von Martuni

Gesammelte Ideen auf Basis von `CLAUDE.md`, `README.md` und `docs/roadmap.md`.

## 1. Suche effizienter machen

- **Aspiration Windows** – Suche mit schmalem Fenster um den vorherigen Score starten; nur bei Fail-High/Low erweitern. Bereits in `docs/aspiration-windows.md` diskutiert.
- **Adaptive Null-Move-Pruning** – Statt fixem `R = 2` z. B. `R = 2 + depth/6` plus **Verification Search**, um Zugzwang-Risiken weiter zu reduzieren. Steht in der Roadmap.
- **Static Exchange Evaluation (SEE)** – Bessere Ordering von Schlagzügen und sicheres Prunen schlechter Schläger. Siehe `docs/see.md`.
- **History Heuristic / Countermove Heuristic** – Leise Züge besser ordnen, damit LMR und Pruning wirksamer werden.
- **Futility Pruning / Razoring / Reverse Futility** – Günstige Abbruchkriterien an den Blättern.
- **Singular Extensions / Multi-Cut** – Selektive Erweiterungen in offensichtlich einzigen Zugstellungen.

## 2. Bewertungsfunktion ausbauen

- **Dynamische Figurenwerte** – Springer/Läufer phasenabhängig, Laufbauer-Anpassungen, dynamischer Läuferpaar-Bonus. Bereits in der Roadmap.
- **Pawn-Structure-Terme** – Doppelte, isolierte, rückständige und Freibauern bewerten.
- **Feinere Mobilität** – Pro Figurentyp und Feld statt nur linearer Safe-Mobility.
- **Besseres King Safety** – Pawn-Storm, Pawn-Shield-Lücken, Flügelangriffe.
- **Bedrohungen / Hängende Figuren** – Ungedeckte Figuren, Angriffe auf höherwertige Steine.
- **Raumvorteil / Outposts / Läufer- vs. Springer-Parameter** je nach Stellungstyp.

## 3. Zeitmanagement und Ausrüstung

- **Bessere Zeitverteilung** – Proportionale Budgetierung mit `MoveOverhead` und Increment statt einfacher gleichmäßiger Aufteilung.
- **TT stärker nutzen** – Für Null-Move-Verifikation und Ponder-Move-Auswahl.
- **Opening-Book verbessern** – Größere/qualitativ bessere Polyglot-Books und ein diverser Zugauswahlmechanismus.

## 4. Mess- und Test-Infrastruktur konsequent nutzen

- Jede Änderung zuerst als **A/B-Test** gegen die aktuelle Version.
- **Regressions- und Progressionstests** in Standardstellungen und Chess960.
- `docs/blunder-analyse.md`-Toolchain nutzen, um gezielt die größten Schwächen anzugehen.

## Kurzfristige Empfehlung

Die nächsten relativ sicheren Elo-Gewinne dürften bei **Aspiration Windows**, **SEE** und **dynamischen Figurenwerten / Läuferpaar-Bonus** liegen, da diese die bestehende Architektur nicht umkrempeln, aber Suchtiefe und Eval-Qualität spürbar verbessern.
