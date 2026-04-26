# Martuni — Ideen zur Spielstärkeverbesserung und Code-Qualität

Basierend auf der Analyse von `CLAUDE.md`, `src/search.rs`, `src/eval.rs` und `src/endgame.rs` ergeben sich folgende konkrete Ansätze für die Weiterentwicklung der Martuni-Engine.

## 1. Spielstärkeverbesserung: Suche & Pruning (Search)

Obwohl die Alpha-Beta-Suche mit Quiescence Search, Iterative Deepening und Transposition Table solide aufgebaut ist, fehlen die meisten fortgeschrittenen Such-Heuristiken (was viel Potenzial für Elo-Gewinne lässt):

*   **Principal Variation Search (PVS) / NegaScout:** 
    Aktuell nutzt `search.rs` durchgehend ein vollständiges Alpha-Beta-Fenster `(-beta, -alpha)` für alle Züge. PVS sucht nur den ersten (besten) Zug mit vollem Fenster. Alle weiteren Züge werden mit einem "Null Window" (Zero Window, `(-alpha - 1, -alpha)`) bewertet. Schlägt die Suche dort fehl (der Zug ist besser als erwartet), wird mit vollem Fenster nachgesucht. Dies reduziert den Suchbaum bei guter Zugsortierung drastisch.
*   **Aspiration Windows:** 
    Anstatt das Iterative Deepening für Tiefe `N` komplett mit dem Fenster `(-INF, INF)` zu starten, sollte ein enges Fenster um die Bewertung der Tiefe `N-1` gelegt werden (z. B. `[last_score - 50, last_score + 50]`). Bei einem Fail-High/Low wird das Fenster vergrößert und die Iteration wiederholt. Das spart ebenfalls viele Knoten.
*   **Late Move Reductions (LMR):** 
    Wie in `search.rs` bereits als "offene Idee" dokumentiert, sollten späte, nicht schlagende und nicht forcierende Züge in der Suchtiefe reduziert (z. B. Tiefe - 1 oder - 2) statt voll durchsucht werden. Dies wirkt der Baumexplosion enorm entgegen.
*   **Null Move Pruning (NMP):** 
    Gemäß Roadmap (`CLAUDE.md`) bereits geplant. Das Aussetzen eines Zuges ("Null Move"), um zu prüfen, ob die Stellung so stark ist, dass sie selbst mit einem Zug Rückstand noch einen Beta-Cutoff erzielt, ist einer der wichtigsten Elo-Bringer moderner Engines.
*   **Futility Pruning & Reverse Futility Pruning (Static Null Move Pruning):** 
    In Knotenpunkten nahe den Blättern (Tiefe 1 oder 2), in denen die statische Bewertung so schlecht ist, dass selbst ein maximaler plausibler Zug (z. B. +300 cp) den Alpha-Wert nicht mehr erreicht, kann die Suche sofort abgebrochen werden.
*   **Move Picker (Lazy Move Generation):** 
    In `order_moves` werden derzeit immer *alle* legalen Züge auf einmal generiert, in einen Vektor gepackt und sortiert. Ein inkrementeller Move Picker, der iterativ arbeitet (erst Hash Move, dann Captures generieren, dann Killer, dann erst Quiet Moves), spart enorm viel Rechenzeit in Knoten, in denen früh ein Cutoff stattfindet.

## 2. Spielstärkeverbesserung: Evaluation

*   **Vollständige getaperte Score-Struktur:** 
    Aktuell sind Material, Piece-Square-Tables (PST) und Mobilität elegant in Middle- und Endgame aufgeteilt. Andere Aspekte (Isolani, Phalanx, Passbauern) fließen nur statisch ein. Ein Freibauer ist im Endspiel aber weitaus mächtiger als im Mittelspiel – diese Werte ebenfalls nach Phase zu interpolieren, bringt deutliche Stärke im Übergang.
*   **Pawn Structure Details:** 
    Es fehlen detaillierte Bauern-Heuristiken wie "Backward Pawns" (rückständige Bauern, die in ihrem Vormarsch blockiert und nicht durch eigene Bauern gedeckt sind) sowie spezifische Boni für Vorposten (Outposts – Felder für Springer/Läufer, die durch Bauern gedeckt und nicht vertrieben werden können).
*   **Bishop Pair Bonus dynamisch:** 
    Das Läuferpaar erhält einen statischen Fixwert (`bishop_pair_each`). Ein dynamischer Bonus, der höher wird, je offener die Stellung ist (z. B. umgekehrt proportional zur Anzahl der Bauern auf dem Brett), bildet die Realität besser ab.
*   **Blockierte Passbauern:** 
    Spezifische Malusse für gegnerische Figuren, die direkt vor einem starken eigenen Passbauern stehen, fehlen noch. 

## 3. Code-Qualität & Refactoring

*   **Dediziertes `Score`-Struct (`struct Score(pub i32, pub i32)`):** 
    In `eval.rs` arbeiten manche Funktionen mit Tuplen `(i32, i32)` für MG/EG (wie `mobility_score`), andere mischen Werte manuell. Ein dediziertes Struct für Middle- und Endgame-Evaluation, das `Add`, `Sub` etc. implementiert, würde `evaluate()` enorm vereinfachen. Die gesamte Logik arbeitet dann mit `Score`, und nur das finale Resultat wird per `.taper(phase)` interpoliert.
*   **Zug-Sortierung Keys (`order_key`):** 
    Die magischen Zahlen in `order_moves` in `search.rs` (`-100_000`, `-50_000`, `-40_000`, etc.) sollten durch benannte Konstanten oder Enum-Stufen (z. B. `OrderStage { HashMove, WinningCapture, Killer, ... }`) ersetzt werden. Das macht den Code sicherer gegen Fehler, wenn neue Heuristiken wie Countermoves hinzukommen.
*   **Iterative Deepening kapseln:** 
    Die Funktion `search()` in `search.rs` baut den Status auf, definiert die Zeitkontrolle und führt den Haupt-Loop für Iterative Deepening aus. Das Auslagern des eigentlichen Schleifenkonstrukts in z. B. `SearchState::iterative_deepening(max_depth)` würde die Hauptfunktion kürzer und übersichtlicher machen.
