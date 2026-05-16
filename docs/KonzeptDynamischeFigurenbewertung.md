# **Konzept: Dynamische Figurenbewertung & Tapering für Martuni**

Die klassische Regel besagt: **Springer lieben geschlossene Stellungen, Läufer lieben offene Stellungen.** Um dies umzusetzen, ohne die Grundwerte (300 cp) global zu verzerren, sollte die Evaluation den Materialwert basierend auf der **Anzahl der verbleibenden Bauern** und der **Spielphase** skalieren.

## **1\. Die Logik der Bauern-Skalierung (Pawn-Count Scaling)**

### **Springer (Knights)**

Ein Springer verliert an Wert, wenn das Brett leerer wird, da er keine weiten Wege zurücklegen kann.

* **Regel:** Bonus für jeden eigenen Bauern auf dem Brett, Malus bei wenigen Bauern.  
* **Best Practice:** Pro Bauer über/unter dem Durchschnitt (8 Bauern) wird der Wert um ca. 2–4 Centipawns angepasst.

### **Läufer (Bishops)**

Ein Läufer gewinnt an Wert, wenn Linien und Diagonalen frei werden.

* **Regel:** Bonus für jeden *fehlenden* Bauern.  
* **Best Practice:** Pro fehlendem Bauer steigt der Wert um ca. 3–5 Centipawns.

## **2\. Phasen-Tapering (Mittelspiel vs. Endspiel)**

Materialbewertung ist kein statischer Zustand. Ein Springer ist im Mittelspiel (MG) oft wertvoller für taktische Manöver, während der Läufer im Endspiel (EG) überlegene Reichweite zeigt.

Wir nutzen eine Phase, die von 24 (Startaufstellung) bis 0 (leeres Brett) sinkt.

* **MG-Wert:** Fokus auf taktische Kooperation mit Bauern.  
* **EG-Wert:** Fokus auf Reichweite und Freibauern-Begleitung.

## **3\. Pseudocode-Implementierung (Integriertes Tapering)**

Dieser Code zeigt, wie Martuni die Werte aus der eval.toml liest und basierend auf der Phase sowie der Bauernstruktur verrechnet.

// Hilfsfunktion für lineares Tapering  
int interpolate(int mg\_val, int eg\_val, int phase) {  
    // phase 24 \= volles MG, phase 0 \= volles EG  
    return (mg\_val \* phase \+ eg\_val \* (24 \- phase)) / 24;  
}

int evaluate\_material(Board board, EvalConfig config, int phase) {  
    int score \= 0;  
    int total\_pawns \= board.total\_pawn\_count();   
    int us\_pawns \= board.pawns\_of\_color(US).count();

    // \--- SPRINGER LOGIK \---  
    // Bonus im MG höher, da Springer im Gewühl besser sind.  
    int n\_count \= board.knights(US).count();  
    int n\_pawn\_adj \= (us\_pawns \- 8\) \* config.knight\_pawn\_scale;  
    int n\_final\_base \= interpolate(config.knight\_mg, config.knight\_eg, phase);  
    score \+= n\_count \* (n\_final\_base \+ n\_pawn\_adj);

    // \--- LÄUFER LOGIK \---  
    // Malus für jeden vorhandenen Bauern (da diese Wege versperren)  
    int b\_count \= board.bishops(US).count();  
    int b\_pawn\_adj \= (8 \- us\_pawns) \* config.bishop\_pawn\_scale;  
    int b\_final\_base \= interpolate(config.bishop\_mg, config.bishop\_eg, phase);  
    score \+= b\_count \* (b\_final\_base \+ b\_pawn\_adj);

    // \--- LÄUFERPAAR DYNAMIK \---  
    if (b\_count \>= 2\) {  
        // Das Läuferpaar ist im Endspiel (Phase \-\> 0\) deutlich mächtiger  
        int mg\_bp \= config.bishop\_pair\_mg;  
        int eg\_bp \= config.bishop\_pair\_eg \+ (16 \- total\_pawns) \* config.bp\_open\_scale;  
        score \+= interpolate(mg\_bp, eg\_bp, phase);  
    }

    return score;  
}

## **4\. Erweiterungsvorschlag für die eval.toml**

Um diese Logik feinjustierbar zu machen, ergänzen wir die bestehende \[material\] Sektion oder erstellen eine neue:

\[material\_dynamic\]  
\# Basiswerte für Tapering (statt nur 300/300)  
knight\_mg \= 310  
knight\_eg \= 290  
bishop\_mg \= 305  
bishop\_eg \= 320

\# Dynamische Skalierung  
knight\_pawn\_scale \= 3   \# cp pro eigenem Bauer  
bishop\_pawn\_scale \= 4   \# cp pro fehlendem Bauer

\# Läuferpaar-Tuning  
bishop\_pair\_mg \= 30     \# Bonus im Mittelspiel (gesamt)  
bishop\_pair\_eg \= 50     \# Basis-Bonus im Endspiel  
bp\_open\_scale  \= 2      \# Extra-Bonus pro fehlendem Bauer (gesamt)

## **Fazit der Einschätzung**

Durch diese Logik wird Martuni:

1. **Im Mittelspiel:** Springer länger behalten, um Druck im Zentrum auszuüben.  
2. **Im Übergang:** Den Abtausch von Bauern forcieren, wenn sie das Läuferpaar besitzt.  
3. **Im Endspiel:** Den "Long-Range"-Vorteil des Läufers mathematisch korrekt gegen die Kurzatmigkeit des Springers abwägen.

Dies behebt die "konservative" Unterbewertung des Läufers, ohne ihn in geschlossenen Stellungen künstlich zu bevorzugen.