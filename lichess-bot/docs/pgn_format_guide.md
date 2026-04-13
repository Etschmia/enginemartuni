# PGN-Format und Metadaten in `lichess-bot` (Martuni)

Diese Handreichung erklärt den detaillierten Aufbau der PGN-Dateien (Portable Game Notation), die in diesem Projekt unter `game_records/` gespeichert werden. Das Verständnis dieses Formats ist besonders wichtig, wenn die PGN-Historie als stetig wachsende, durchsuchbare Datenbank (z.B. für Auswertungen, Machine Learning oder Statistiken) genutzt werden soll.

## 1. Metadaten (PGN-Header / Tags)

Am Anfang jeder PGN-Datei stehen die sogenannten "Tags", markiert mit eckigen Klammern `[...]`. Sie enthalten die Metadaten zur Partie. Ein Teil dieser Daten kommt direkt von der Lichess-API, ein anderer Teil wird vom internen PGN-Builder des `lichess-bot` (z. B. in `lib/lichess_bot.py`) ergänzt.

**Wichtigste Standard-Tags:**
*   `[Event "rated rapid game"]`: Art des Spiels. Sehr nützlich, um später nach Bedenkzeiten oder "Rated / Casual" zu filtern.
*   `[Site "https://lichess.org/..."]`: Der direkte Link zur Partie auf Lichess. Dient als eindeutiger Identifikator und Referenz-URL.
*   `[Date "YYYY.MM.DD"]` / `[UTCDate "YYYY.MM.DD"]` / `[UTCTime "HH:MM:SS"]`: Datum und Uhrzeit der Partie in lokaler und UTC-Zeit.
*   `[White "Name"]` / `[Black "Name"]`: Usernames oder Bot-Namen der Spieler.
*   `[Result "0-1"]`: Ergebnis (`1-0` = Weiß gewinnt, `0-1` = Schwarz gewinnt, `1/2-1/2` = Remis, `*` = noch laufend/abgebrochen).
*   `[GameId "..."]`: Die eindeutige Lichess-Game-ID (wichtig als Primary Key in einer Datenbank!).

**Erweiterte Lichess-Metadaten:**
*   `[WhiteElo "2300"]` / `[BlackElo "1860"]`: Die Wertungszahlen vor der Partie.
*   `[WhiteRatingDiff "-700"]` / `[BlackRatingDiff "+6"]`: Veränderung des Elo-Ratings nach der Partie.
*   `[WhiteTitle "BOT"]` / `[BlackTitle "BOT"]`: Zeigt an, ob ein Spieler einen Bot-Account (`BOT`), Großmeister (`GM`), Master (`IM`, `FM`), etc. hat.
*   `[Variant "Standard"]`: Gespielte Schachvariante (z. B. Standard, Chess960, Atomic).
*   `[TimeControl "900+10"]`: Zeigt die initiale Zeit und das Inkrement pro Zug in Sekunden.
*   `[ECO "E10"]` / `[Opening "..."]`: Von Lichess erkannte Eröffnung (Encyclopaedia of Chess Openings Code) und der Eröffnungsname (z. B. "Indian Defense").
*   `[Termination "Normal"]`: Wie endete das Spiel? (z.B. `Normal` = Matt oder Aufgabe, `Time forfeit` = Zeitablauf).

## 2. Inline-Kommentare (Züge und Lichess/Engine-Annotationen)

Innerhalb des PGN-Bodys wird die Zugfolge gemäß der SAN-Notation (Standard Algebraic Notation) abgebildet. Um Metadaten exakt  mit einzelnen Zügen zu verknüpfen, werden `{ [%schlüsselwort wert] }` Kommentare verwendet.

### A. Die Uhrzeit: `[%clk HH:MM:SS]`
Zeigt die verbleibende Bedenkzeit des Spielers _nach_ Ausführung des Zuges an.
*   **Beispiel**: `1... Nf6 { [%clk 0:15:00] }`
*   **Herkunft**: Diese Daten werden von Lichess beim Abspielen der Partie an deinen Bot übertragen. Lichess (als Schiedsrichter) ist die Quelle der Wahrheit für die Uhr.
*   **Parsing-Empfehlung**: In einer Datenbank lässt sich aus der Differenz zweier `%clk`-Werte die Zeit berechnen, die für einen konkreten Zug nachgedacht wurde (Abzüglich Inkrement/Lag-Puffer!).

### B. Engine-Bewertung: `[%eval wert,tiefe]`
Die Einschätzung der Stellung in dem Moment.
*   **Format (`score, depth`)**: `[%eval 0.15,3]` bedeutet, die Stellung wird mit `+0.15` (im Vorteil für Weiß) auf einer Suchtiefe (Depth) von `3` Halbzügen bewertet.
*   **Matt-Format (`#Züge`)**: `[%eval #-3,2]` bedeutet Matt (für Schwarz) in erzwungenen 3 Zügen, gefunden bei Suchtiefe 2. Ein positiver Wert (z. B. `#4`) steht für Matt für Weiß.
*   **Herkunft**: Diese Werte stammen – bei den Zügen des Bots – **direkt aus deiner Engine (Martuni)**. Das `lichess-bot` Skript fängt die UCI-Werte der Engine (`info score cp ... depth ...`) während sie rechnet ab und speichert sie gezielt im PGN ab (`lib/lichess_bot.py → set_eval`).

### C. Variationen (Principal Variation - PV)
Im File sieht man häufig folgendes Muster:
> `6. e4 { [%clk 0:14:33] } ( 6. e4 { [%eval 0.15,3] } )`

Was auf den ersten Blick doppelt aussieht, ist ein Lichess-/PGN-Mechanismus für Bot-Analysen:
1.  Der **Haupt-Knoten** `6. e4 { [%clk 0:14:33] }` repräsentiert den tatsächlich auf dem Brett ausgeführten Zug und die von Lichess vergebene Zeit.
2.  Die in runden Klammern geschachtelte **Variation** `( 6. e4 { [%eval 0.15,3] } )` repräsentiert die von der Engine geplante "Principal Variation" (PV) für genau diesen Zug. In `lichess-bot.py` wird die Engine-Evaluation der Variation als Unter-Knoten zum Hauptspielverlauf hinzugefügt.
*   **Szenario:** Hätte die Engine einen alternativen Zug als besten angesehen, die Partie aber anders verlief, würde hier der erwartete Linienverlauf der Engine stehen.


### Gibt es weitere Inline-Schlüsselwörter?
Die Lichess/lichess-bot Kombination fügt standardmäßig hauptsächlich `%clk` und `%eval` ein. Die offizielle PGN-Spezifikation und `python-chess` erlauben jedoch noch weitere (die je nach Engine-Funktionalität künftig hinzukommen könnten):
*   `[%emt HH:MM:SS]` (Elapsed Move Time): Die exakte Zeit, die für diesen Zug verbraucht wurde (wird oft alternativ/ergänzend zu `%clk` von anderen Plattformen genutzt).
*   `[%csl R... / %cal G...]` (Color Square/Arrow): Grafische Hervorhebungen auf dem Schachbrett, die manche Engines ausgeben können (farbige Pfeile auf Lichess). Werden aktuell von Martuni nicht verwendet, können aber bei PGN-Dateien von Online-Analysen auftauchen.

---

## 3. Tipps zum Aufbau einer durchsuchbaren Datenbank

Wenn die PGN-Sammlung stündlich wächst und strukturiert verwertet werden soll:

1.  **Nutze Standard-Bibliotheken (Keine Regex!)**:
    Anstatt das PGN-Format mit Regulären Ausdrücken auszulesen, verwende `python-chess` (was im Lichess-Bot bereits enthalten ist).
    ```python
    import chess.pgn
    with open("game_records/Martuni vs shaheris - bkaXYnkn.pgn") as pgn:
        game = chess.pgn.read_game(pgn)
        
        # Metadaten lesen:
        event = game.headers["Event"]
        
        # Durch Züge iterieren:
        for node in game.mainline():
            uhrzeit_danach = node.clock() # liefert float in Sekunden
            
            # Die Evaluation steht im PGN häufig im PV-Zweig (Variation):
            if len(node.variations) > 0:
                eval_obj = node.variations[0].eval()
                if eval_obj is not None:
                    print("Score:", eval_obj.pov(chess.WHITE).score(mate_score=10000))
    ```
2.  **Inkrementelles Einlesen**:
    Wenn ein Skript periodisch neue PGNs in deine Datenbank laden soll, orientiere dich am `[GameId "XYZ"]` Header. Er eignet sich am besten als "Primary Key" (oder Unique Identifier).
3.  **Zeit & CPU entkoppeln**:
    Das Einlesen (Parsen) tausender Schachpartien in Python kann Rechenleistung kosten. Für ein stündliches Cronjob-Setup sollte man sich nur die "neu hinzugefügten/geänderten" Dateien seit dem letzten Run anschauen anstatt jedes Mal das gesamte `/game_records` Verzeichnis neu zu parsen.
