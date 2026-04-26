# Martuni: Detaillierter Verbesserungsplan

Dieser Plan skizziert die technischen Details für zukünftige Verbesserungen. 

> [!NOTE]
> **Aktueller Status:** Es werden zunächst mehrere hundert Partien (Lichess/Lokale Evaluierung) mit dem derzeitigen Stand gespielt, um eine saubere Baseline zu generieren und die neuesten Anpassungen (z.B. King-Exposure Entschärfung) fundiert auszuwerten. **Es werden im Moment keine Code-Änderungen vorgenommen.** Erst nach der Validierung werden die folgenden Phasen schrittweise umgesetzt.

---

## Phase 1: Code-Qualität & Refactoring (Grundlagen schaffen)

Bevor neue komplexe Such-Logik hinzukommt, wird die bestehende Code-Basis in `src/eval.rs` und `src/search.rs` robuster und übersichtlicher gemacht.

### 1.1 Einführung eines `Score`-Structs für Tapered Eval
* **Ziel:** Vermeidung von unübersichtlichen `(i32, i32)` Tupeln und manueller Mischung von MG/EG (Middle Game / End Game) Werten.
* **Umsetzung:** 
  - Erstellen eines neuen Typen `pub struct Score(pub i32, pub i32);`.
  - Implementierung der Standard-Traits `Add`, `Sub`, `AddAssign`, `SubAssign`, `Mul<i32>`, `Div<i32>`.
  - Anpassung aller Evaluierungs-Funktionen (`evaluate_side`, `mobility_score`, `pst_score`, etc.), sodass diese durchgängig ein `Score`-Objekt zurückgeben und aufbauen.
  - Erst ganz am Ende von `evaluate(board, p)` wird das aufsummierte `Score`-Objekt mittels einer Hilfsfunktion `score.taper(phase)` in den finalen `i32`-Score konvertiert. Dies macht die Ausweitung der Tapered-Logik trivial.

### 1.2 Refactoring der Zug-Sortierung (`order_moves`)
* **Ziel:** Magische Konstanten (`-100_000`, `-50_000`) eliminieren und die Sortierung zukunftssicher machen.
* **Umsetzung:**
  - Einführung benannter Konstanten (oder Enums) für die Score-Klassen der Züge:
    ```rust
    const ORDER_HASH: i32 = -100_000;
    const ORDER_PROMO_QUEEN: i32 = -50_000;
    const ORDER_WIN_CAPTURE_BASE: i32 = -40_000;
    const ORDER_KILLER_1: i32 = -30_000;
    // ...
    ```
  - **Perspektivisch:** Umstellung auf einen statusbasierten `MovePicker` (Lazy Move Generation). Anstatt alle Züge vorab in einen Vektor zu generieren und zu sortieren, arbeitet der Picker schrittweise: 1. Hash Move prüfen, 2. Captures generieren, SEE bewerten und prüfen, 3. Killer Moves prüfen, 4. Quiet Moves generieren. Das spart sehr viele CPU-Zyklen in Knotenpunkten mit frühem Cutoff.

### 1.3 Iterative Deepening auslagern
* **Ziel:** Die `search()`-Hauptfunktion in `search.rs` entschlacken.
* **Umsetzung:**
  - Verlagerung des Loops `for depth in 1..=max_depth { alpha_beta(...) }` in eine eigene Methode des `SearchState` (z.B. `SearchState::run_iterative_deepening(board, max_depth)`).
  - `search()` ist dann nur noch für das Parsing der Parameter, die Initialisierung des Status, Polyglot-Buch-Abfragen und die Fallback-Sicherheit zuständig.

---

## Phase 2: Search & Pruning (Elo-Gewinne)

Nachdem das Refactoring sitzt, werden gezielte Such-Heuristiken implementiert. Diese werden einzeln hinzugefügt und gegen den Baseline-Commit getestet.

### 2.1 Principal Variation Search (PVS) / NegaScout
* **Umsetzung:**
  - In `alpha_beta` wird nur der allererste Zug in einem Knoten normal mit dem Fenster `(-beta, -alpha)` durchsucht.
  - Für alle folgenden Züge wird eine Null-Fenster-Suche (Zero-Window) mit `(-alpha - 1, -alpha)` durchgeführt, da wir davon ausgehen, dass der erste Zug bereits der beste ist.
  - Falls das Ergebnis dieser Null-Fenster-Suche unerwartet größer als `alpha` und kleiner als `beta` ist (ein besserer Zug wurde gefunden), wird mit dem vollen Fenster `(-beta, -alpha)` noch einmal sicherheitshalber nachgesucht ("Re-Search").

### 2.2 Null Move Pruning (NMP)
* **Umsetzung:**
  - Zu Beginn von `alpha_beta` prüfen wir drei Konditionen: (1) Sind wir nicht im Schach? (2) Ist Tiefe `>= 3`? (3) Haben wir noch Figuren auf dem Brett, die keine Bauern sind (Schutz gegen Zugzwang in Bauernendspielen)?
  - Falls ja, führen wir auf dem Board den "Null-Zug" (Farbwechsel ohne Bewegung) aus und starten rekursiv eine Suche mit verringerter Tiefe (`depth - 1 - R`, wobei `R` typischerweise 2 oder 3 ist) und einem Zero-Window `(-beta, 1 - beta)`.
  - Schlägt diese Suche fehl (Ergebnis `>= beta`), steht der Gegner trotz seines Extrazuges so schlecht, dass wir einen sofortigen Beta-Cutoff erzeugen können (`return beta;`).

### 2.3 Late Move Reductions (LMR)
* **Umsetzung:**
  - Werden in `alpha_beta` nach den ersten z.B. 3 Zügen ruhige Züge ("Quiet Moves": keine Captures, keine Promotions) geprüft und wir stehen nicht im Schach, wird die Suchtiefe künstlich um 1 oder 2 reduziert.
  - Schlägt diese reduzierte Suche über `alpha` hinaus an, wird mit der regulären Tiefe voll nachgesucht.
  - In Kombination mit PVS schrumpft die Suchbreite in Randknoten enorm zusammen.

### 2.4 Aspiration Windows
* **Umsetzung:**
  - In der Iterative-Deepening-Schleife startet das Fenster nicht mehr blind mit `(-INF, INF)`.
  - Stattdessen nutzen wir den `best_score` der vorherigen Tiefe `d-1` und setzen das anfängliche Suchfenster für Tiefe `d` auf ein enges Band, z. B. `[last_score - 50, last_score + 50]`.
  - Fällt das Ergebnis aus diesem Fenster heraus ("Fail-Low" oder "Fail-High"), wird das Fenster drastisch ausgeweitet (z.B. `[last_score - 200, INF]`) und die Tiefe wird neu durchsucht. Da ca. 80-90% der Suchen ins Band fallen, ist die Zeitersparnis enorm.

---

## Phase 3: Evaluation (Tapering & fortgeschrittene Heuristiken)

### 3.1 Dynamisches Tapering für Bauern und Strukturen
* **Umsetzung:**
  - Aktuelle statische Boni (Isolani-Malus, Phalanx, Passbauer) in `eval.rs` werden auf das neue `Score`-Struct umgeschrieben.
  - Ein Passbauer erhält im Endspiel (EG) einen drastisch ansteigenden Wert, während der Mittelspiel-Wert (MG) moderater bleibt, da er dort leichter geblockt und belagert werden kann.

### 3.2 Backward Pawns & Outposts
* **Umsetzung:**
  - **Backward Pawns (Rückständige Bauern):** Ein Bauer hat keinen benachbarten eigenen Bauern mehr hinter sich oder neben sich, und das Feld vor ihm wird sicher vom Gegner kontrolliert. Dafür wird in `eval.rs` ein expliziter `Score`-Malus hinzugefügt.
  - **Outposts (Vorposten):** Vor allem für Springer! Ein Springer, der sich auf den zentralen Reihen 4–6 befindet, von einem eigenen Bauern gedeckt wird und *nicht* mehr durch gegnerische Bauern vertrieben werden kann (weil es auf den benachbarten Linien keine Vorwärtsbauern des Gegners mehr gibt), erhält einen massiven `Score`-Bonus. Dies belohnt hochstrategisches Positionsspiel.

---

## Ablauf und Validierung
1. **Baseline-Evaluierung abschließen:** 200–500 Testpartien auf Lichess (Blitz / Rapid) generieren.
2. **Phase 1 umsetzen:** Refactoring durchführen. Da dies rein struktureller Natur ist, darf sich die Metrik (`bench` Nodes) überhaupt nicht ändern! Es dient nur dem sauberen Code.
3. **Phasen 2 & 3 umsetzen:** Features einzeln programmieren, z.B. als erstes NMP. Lokales Self-Play-Turnier gegen den Baseline-Commit spielen lassen (z.B. 100 Partien auf kurzer Zeitkontrolle, z.B. 10+0.1). Nur bei positivem Elo-Gain übernehmen.
