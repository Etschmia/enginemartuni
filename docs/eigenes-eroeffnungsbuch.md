# Eigenes Eröffnungsbuch für Martuni — Konzept

Stand 30.09.2026. Konzept und Datenlage, noch kein Code. **Tobias hat am
30.09. entschieden** (Abschnitt 6), das Format ist geklärt (Abschnitt 7). Anlass: Spark hat sich ein eigenes Buch aus eigenen
Partien und eigener Analyse gebaut (tmux-Sitzung `Spark`,
`~/sparkengine/book/funken.book`, `MESSERGEBNISSE.md` Abschnitt 9.19).
Frage von Tobias: Soll Martuni das auch tun, damit seine tatsächlichen
Stärken und Schwächen in das Buch einfließen, statt fremder Bücher aus dem
Internet?

Zahlen erzeugt mit `tools/book_audit.py` (nur PGN-Parsing, ein Kern, etwa
1 Minute).

## 1. Was Spark gemacht hat

- 245 eigene Lichess-Partien → 78 Frühstellungen, die mindestens 6-mal
  vorkamen. Für jede Stellung eine Tiefenanalyse mit der eigenen Engine
  (`go nodes 2000000`), gespeichert werden der beste Zug und bis zu zwei
  weitere, wenn sie höchstens 50 cp schlechter sind. Kein fremdes Buch,
  kein Stockfish.
- Eigenes Textformat (FEN; Zugliste), eigenes Modul `src/book.rs`,
  zufällige Auswahl unter den gespeicherten Zügen.
- Messung M10: Buch gegen kein Buch, 200 Partien bei festen 200k Knoten,
  **+70 Elo (CI +39…+103)**.

### Warum sich die +70 nicht auf Martuni übertragen lassen

1. **Spark hatte vorher gar kein Buch.** Gemessen wurde also „Buch gegen
   nichts". Martuni hat schon drei Bücher (gm2001, komodo, rodent). Ein
   eigenes Buch müsste diese schlagen, nicht das Nichts.
2. **Die Buchzüge sind zehnmal tiefer gerechnet als die Züge im Match**
   (2M gegen 200k Knoten). Ein Teil der +70 ist schlicht eingefrorene
   Rechenzeit.
3. **Das Selfplay-Match war gegen einen deterministischen Gegner.** Der Gewinn
   kommt fast vollständig aus einer einzigen Familie („nach 1.d4 18:0").
   Das Buch hat also eine Linie gefunden, in der die Basisversion jedes Mal
   denselben Fehler macht. Gegen Lichess-Gegner mit eigenen Büchern ist das
   viel weniger wert. Spark schreibt selbst, dass die Stichprobe pro Linie zu
   klein für Aussagen über einzelne Linien ist und dass das Weiß-Repertoire
   (2:6) neutral bis negativ ist.

Die Idee dahinter ist gut: eigene Daten, eigene Analyse, also Eigenleistung.
Die Messlatte liegt für Martuni aber höher.

## 2. Datenlage Martuni

### 2.1 Ein unfreiwilliges Experiment: 18 Tage ohne Buch

Nebenbefund dieser Untersuchung: Vom **06.09. bis 24.09.2026** hat Martuni
live **ohne Eröffnungsbuch** gespielt. Eine fremde `.env` in `~/lichess-bot`
verdeckte `BOOK_FILES` (behoben mit `845708b`, das stand bisher nicht in der
Roadmap). Das Audit bestätigt das sauber. Vorher und nachher folgt Martuni
in 100 % der Partien dem Buch, im Zeitraum dazwischen weicht es in 481 von
620 Partien davon ab.

| Zeitraum | Partien | Score | erwartet (Elo) | Leistung | Ø Buchausgang |
|---|---:|---:|---:|---:|---:|
| A vor 06.09. (Buch) | 292 | 41,4 % | 37,6 % | +28 Elo | Halbzug 16 |
| B 06.–24.09. (ohne Buch) | 620 | 46,7 % | 48,2 % | −10 Elo | Halbzug 7 |
| C nach Fix (Buch) | 204 | 49,5 % | 47,0 % | +18 Elo | Halbzug 16 |

„Erwartet" ist der Score, der sich aus der Elo-Differenz jeder Partie ergibt.
**Mit Buch spielt Martuni rund 20–40 Elo über der Erwartung, ohne Buch leicht
darunter.** Das ist grob, denn in den Zeitraum fallen auch die Rollouts PR #4
(11.09.) und SEE-Fix (22.09.), und die Konfidenzintervalle liegen bei etwa
±40 Elo. Die Richtung passt aber zu Sparks Ergebnis. Für uns heißt das:
**Die jetzigen fremden Bücher sind grob 30 Elo wert. Das ist die Latte, die
ein eigenes Buch überspringen muss.**

### 2.2 Wo Martunis Eröffnungsschwächen liegen

Linien (erste zwei Halbzüge, Score minus Erwartung, n ≥ 20):

| Linie | mit Buch | ohne Buch |
|---|---|---|
| Schwarz nach 1.e4 e5 | +2,0 (n 35) | **−8,6 (n 140)** |
| Schwarz nach 1.d4 d5 | **−11,0 (n 23)** | – |
| Schwarz nach 1.d4 Nf6 | −4,3 (n 39) | −7,2 (n 92) |
| Weiß 1.e4 e5 | **+12,4 (n 46)** | **−14,5 (n 27)** |
| Weiß 1.e4 c5 | **+15,0 (n 24)** | – |
| Weiß 1.d4 d5 | +4,0 (n 31) | +2,2 (n 90) |

Beobachtungen, alle mit kleinen Stichproben:

- **Ohne Buch wählt Martuni als Weiß fast immer 1.d4.** Die eigene Suche
  hat eine klare Vorliebe. Mit Buch spielt Martuni auch 1.e4 und ist dort
  bisher *besser* (+12 bis +15). Das spricht eher gegen ein Buch, das nur
  die eigene Tiefenanalyse abbildet, denn es würde Martuni auf seine
  Lieblingslinie festlegen.
- **Konkrete Falle, die Martunis eigene Suche nicht sieht:** Nach
  1.e4 e5 2.Nf3 Nc6 3.Bb5 Nf6 4.O-O Bc5 5.Nxe5 Nxe5 6.d4 c6 7.dxe5 Nxe4
  8.Bd3 spielte Martuni ohne Buch **5-mal 8…Nxf2??** (SF: −0,6 → −2,1;
  Partien vom 07., 17., 20., 21. und 21.09.). rodent.bin kennt hier 8…d5,
  was auch SF als besten Zug sieht. Mit Buch wäre das nicht passiert. Das
  zeigt die Grenze von Sparks Methode: Ein Buch aus der eigenen
  Tiefenanalyse hätte hier womöglich genau 8…Nxf2 gespeichert.
- **43 von 51 frühen Blundern (Halbzug ≤ 20) passieren nach dem
  Buchausgang, 31 davon innerhalb von 8 Halbzügen.** Der riskanteste
  Moment ist also der Übergang vom Buch zur eigenen Suche. Das passt zum
  Chess960-Befund, wo es gar kein Buch gibt (ply≤16-Blunder 10× Standard).

### 2.3 Wo fremde Bücher fehlen: Varianten

Für Three-check, King of the Hill, Horde, Racing Kings, Atomic, Crazyhouse
und Antichess hat Martuni **gar kein Buch**, und im Internet gibt es kaum
brauchbare. Der Lookback vom 22.09. fand in Three-check die Falle 1.Nc3 e5
2.e3 Nf6 3.Bc4 Nc6?? (14-mal) und in KotH 26 von 26 Verlusten per
Königsmarsch. **Dort ist ein eigenes Buch nicht nur Feinschliff, sondern
die einzige Buchquelle.** Das Potenzial ist wahrscheinlich größer als im
Standardschach.

## 3. Optionen

### A — Sparks Weg: Buch aus eigener Tiefenanalyse
Häufige Frühstellungen aus eigenen Partien sammeln, jede mit Martuni lange
rechnen (z. B. 30–60 s), die besten Züge als Buch speichern.
- **Plus:** reine Eigenleistung, einfach. Spart im Blitz Zeit in der
  Eröffnung, die Züge sind tiefer gerechnet als im Spiel.
- **Minus:** Das Buch weiß nie mehr als Martuni. Eigene Eval-Fehler werden
  eingefroren, statt korrigiert (siehe Nxf2). Die Vielfalt schrumpft auf
  Martunis Lieblingslinien, und Gegner-Bots mit Lernfunktion könnten sie
  ausnutzen.

### B — Ergebnis-Lernen auf den vorhandenen Büchern
Die bisherigen Bücher bleiben, die Gewichte werden aber nach Martunis
tatsächlichem Lichess-Ergebnis in jeder Linie angepasst: Linien, in denen
Martuni überdurchschnittlich punktet, werden hochgewichtet, schlechte
abgewertet oder gestrichen (Polyglot hat dafür sogar ein `learn`-Feld).
- **Plus:** Das ist das, was „Stärken und Schwächen im Buch" am direktesten
  meint. Die Daten fallen ohnehin an, und es kostet kaum Rechenzeit.
- **Minus:** Die Stichproben pro Linie sind klein (siehe Tabelle, n 20–50).
  Man braucht eine vorsichtige Statistik (z. B. Bayes-Schätzer mit
  Elo-Erwartung als Prior), sonst jagt man Rauschen. Das Buch lernt nur in
  Linien, die das Buch schon kennt.

### C — Hybrid: eigener Baum mit Schiedsrichter und Ergebnis-Lernen (Empfehlung)
Ein eigenes Buch `martuni.bin`, gebaut in drei Schichten:
1. **Kandidaten:** Züge aus den vorhandenen Büchern *und* Martunis eigener
   Tiefenanalyse (so wie bei Spark) für alle Stellungen, die in eigenen
   Partien mindestens k-mal vorkamen.
2. **Schiedsrichter:** Jeder Kandidat wird über die Grok-Schleuse mit
   Stockfish geprüft (die Infrastruktur steht). Züge, die dabei mehr als
   X cp verlieren, fliegen raus. Das fängt Fälle wie Nxf2. Stockfish
   bestimmt keine Züge, er legt nur ein Veto ein. Das passt zu unserer
   Eigenleistungsregel, genau wie die Blunder-Analyse.
3. **Gewichte nach eigener Stärke:** Unter den verbleibenden Zügen
   entscheidet Martunis Ergebnis in der Linie (Option B). Zusätzlich zählt,
   wie gut Martunis Eval nach Buchausgang mit Stockfish übereinstimmt:
   Stellungen, in denen Martuni sich „richtig einschätzt", sind
   Martuni-Stellungen, Stellungen mit großer Eval-Drift sind es nicht.
   Hier fließen die Stärken und Schwächen tatsächlich ein.
- **Schwerpunkt Buchausgang:** Da die meisten frühen Blunder kurz nach dem
  Buchausgang passieren, sollte das Buch gezielt an den häufigsten
  Ausgangsstellungen 2–4 Halbzüge weiter reichen. Es sollte also nicht
  breiter werden, sondern dort tiefer, wo Martuni tatsächlich rausfällt.
- **Varianten:** Dieselbe Pipeline mit Fairy-Stockfish als Schiedsrichter
  (steht auf dem Grok-Bot schon bereit). Das hat vermutlich den größten
  Hebel, besonders für Three-check und KotH.

### D — Buch aus Selfplay-Statistik
Viele Selfplay-Partien aus vielen Startlinien spielen und Linien nach
Ergebnis bewerten. Das wäre die gründlichste Methode, braucht aber
Tausende Partien und damit genau die Rechenleistung, die gerade fehlt.
**Zurückgestellt.**

## 4. Messung

- **Selfplay-A/B mit UHO-Stellungen misst ein Buch nicht**, denn fastchess
  startet dort schon aus Buchstellungen. Möglich sind Selfplay aus der
  Grundstellung (Martuni mit neuem Buch gegen Martuni mit den alten
  Büchern, Zufallsauswahl an) oder ein Gauntlet gegen andere Engines. Der
  deterministische Effekt von Spark (eine Linie mit 18:0) ist dabei eine
  Falle. Beide Seiten brauchen Zufall.
- **Lichess ist die eigentliche Messung**, mit derselben Methode wie oben
  (Score gegen Elo-Erwartung, getrennt nach Linie). Das Audit-Skript ist
  dafür schon da. Nachteil: Es braucht einige hundert Partien.
- Rollback ist trivial: `BOOK_FILES` in der `.env` zurückstellen und
  neu starten. Die Engine ändert sich dafür nicht.

## 5. Vorgeschlagene Reihenfolge

| Phase | Inhalt | Rechenlast |
|---|---|---|
| 0 | `tools/book_audit.py` (erledigt), Befund in die Roadmap | keine |
| 1 | **Varianten-Buch Three-check + KotH**, klein: nur die häufigsten 20–40 Frühstellungen aus eigenen Partien, Martuni-Analyse + Fairy-SF-Veto | gering (Analyse läuft auf dem Grok-Bot) |
| 2 | **Standard: `martuni_patches.bin` zum Ausgangsbuch ausbauen** — Martuni-Züge an den 30–50 häufigsten Buchausgangsstellungen (+2–4 Halbzüge), SF-Veto; alte Bücher bleiben dahinter | gering bis mittel |
| 3 | **Ergebnis-Lernen** über allen Buchstellungen (Option B), regelmäßig nach jeder Auswertung | gering |
| 4 | Vollständiges `martuni.bin` ersetzt gm2001/komodo/rodent, nur wenn Phase 2/3 live einen Vorteil zeigen | mittel |

Sparks Buch-Experiment ist seit dem 30.09. abgeschlossen, der Server hätte
also wieder Kapazität für die Martuni-Tiefenanalysen in Phase 1 und 2. Die
Stockfish-Prüfung läuft ohnehin über die Schleuse auf dem Grok-Bot. Die Entscheidungen
stehen in Abschnitt 6, Start mit Three-check/KotH (Abschnitt 8).

## 6. Entscheidungen (Tobias, 30.09.2026)

1. **Stockfish darf schlechte Züge streichen** (Veto über die Grok-Schleuse,
   Fairy-Stockfish für die Varianten). Stockfish schlägt selbst keine Züge vor.
2. **Format:** Polyglot bleibt, *es sei denn*, ein eigenes Format ist schnell
   aufgesetzt *und* zur Laufzeit billiger. Ausgewertet in Abschnitt 7.
3. **Reihenfolge:** zuerst Three-check und King of the Hill, dann Standard.
   **Chess960 gar nicht** (kein Buch sinnvoll, Fass ohne Boden).
4. **Vielfalt:** Zwei Züge pro Buchstellung sind das absolute Minimum.
   Stellungen, in denen nach dem Stockfish-Veto nur ein Zug übrig bleibt,
   kommen nicht ins Buch, dort rechnet Martuni selbst.

## 7. Brauchen wir Polyglot? (Formatfrage)

### Laufzeit: kein Argument, weder dafür noch dagegen

- **Abfrage pro Zug:** einmal an der Wurzel (`search.rs:335`). Aufwand:
  ein Zobrist-Hash (rund 40 XOR) plus eine binäre Suche, bei komodo.bin mit
  578k Einträgen etwa 20 Vergleiche. Das dauert Mikrosekunden, bei
  Sekunden Bedenkzeit pro Zug. Ein eigenes Format kann hier nichts sparen,
  was man messen könnte.
- **Laden beim Start:** gemessen 30.09. mit allen vier Büchern (12,5 MB,
  rund 790k Einträge) **ca. 180 ms**, ohne Bücher ca. 35 ms. Das kostet also
  rund 150 ms einmal pro Engine-Start, nicht pro Zug. Ein eigenes Buch mit
  einigen tausend Einträgen lädt in deutlich unter 1 ms, *egal in welchem
  Format*. Die Ersparnis kommt daher, dass die großen fremden Bücher
  wegfallen, nicht vom Format.

### Das eigentliche Argument: Three-check passt nicht in Polyglot

Ein Polyglot-Schlüssel kennt nur Figuren, Zugrecht, Rochade und en passant.
Die **Zahl der noch fehlenden Schachgebote** fehlt. In Three-check sind
„gleiche Figurenstellung, Weiß hat schon 2 Schach gegeben" und „…noch keins"
aber völlig verschiedene Stellungen. Geprüft: python-chess liefert für beide
denselben Polyglot-Hash. Ein normales Polyglot-Buch würde sie verwechseln.

### Empfehlung: Polyglot-Datei, eigener Schlüssel

- **Dateiaufbau bleibt Polyglot:** 16-Byte-Einträge (Schlüssel, Zug,
  Gewicht, Lern-Feld), nach Schlüssel sortiert. Der vorhandene Loader
  (`src/polyglot/book.rs`, `find` per binärer Suche) funktioniert
  unverändert. Es gibt also keinen neuen Loader und kein neues Format.
- **Schlüssel = Zobrist-Hash von shakmaty** (`zobrist_hash`, shakmaty 0.29).
  Laut dessen eigenem Test ist er für normales Schach **identisch mit dem
  Polyglot-Hash** (`test_polyglot`). Für Three-check verrechnet er
  zusätzlich die fehlenden Schachgebote (`zobrist_for_remaining_checks`).
  KotH hat keinen Zusatzzustand, der Schlüssel entspricht dort Polyglot.
- **Pro Variante eine eigene Datei** (z. B. `martuni_3check.bin`,
  `martuni_koth.bin`, später `martuni.bin` für Standard). Das ist ohnehin
  nötig, weil in KotH und Standard dieselbe Stellung denselben Schlüssel hat,
  aber andere Züge richtig sind.
- **Das Bau-Werkzeug muss in Rust geschrieben sein** und dieselbe
  Hash-Funktion wie die Engine nutzen, etwa als `src/bin/bookbuild.rs`.
  Dann sind Engine und Buch garantiert schlüsselgleich. python-chess scheidet
  als Bau-Werkzeug aus, weil es den Three-check-Zähler nicht verrechnet
  (siehe oben). Die Statistik (welche Stellungen, welche Ergebnisse) darf in
  Python bleiben, das Schreiben der `.bin` nicht.
- **Kompatibilität:** Das Standard-Buch bleibt mit anderen Polyglot-Tools
  lesbar, das ist ein netter Nebeneffekt. Die Varianten-Bücher kann nur
  Martuni lesen, was nach der Vorgabe auch nicht nötig ist.

Fazit: **Polyglot brauchen wir nicht wegen der Kompatibilität.** Wir
behalten den Dateiaufbau, weil er fertig ist, schnell genug ist und nichts
kostet. Der einzige Unterschied liegt beim Schlüssel, und der kommt
geschenkt aus shakmaty.

### Was sich an der Engine ändern muss (Tobias)

1. `search.rs:335` fragt das Buch nur für normales Schach ab
   (`req.board.as_std()`). Für die Varianten braucht es eine Abfrage über
   das Backend-Board mit dem shakmaty-Hash.
2. Konfiguration je Variante (z. B. `BOOK_FILES_3CHECK`,
   `BOOK_FILES_KOTH`), weil heute ein einziges `BOOK_FILES` für normales
   Schach gilt.
3. Züge im Buch werden wie bisher gegen die legalen Züge geprüft
   (`decode_move`). Das funktioniert für Three-check und KotH, weil dort
   keine Einsetzzüge vorkommen. Crazyhouse (Drops) würde nicht passen, ist
   aber nicht geplant.
4. Standard, später: Wenn `martuni.bin` die fremden Bücher ersetzt, bleibt
   der Hash dort der Polyglot-Hash der `chess`-Crate. Das Bau-Werkzeug nutzt
   für Standard dieselbe Funktion (`src/polyglot/hash.rs`) oder shakmaty,
   beide sind für Standard identisch.

## 8. Nächster Schritt: Three-check und KotH

1. **Stellungen sammeln** (Python, Statistik): Frühstellungen aus eigenen
   Live-Partien je Variante, die mindestens k-mal vorkamen, dazu Martunis
   Ergebnis pro Stellung und Zug.
2. **Martuni-Tiefenanalyse** je Stellung (MultiPV bzw. mehrere
   Kandidaten), jetzt möglich, da der Server frei ist.
3. **Veto** über die Schleuse (Fairy-Stockfish): Züge mit großem Verlust
   raus. Weniger als 2 Züge übrig bedeutet: Stellung nicht ins Buch.
4. **Gewichte** aus Analyse und Live-Ergebnis. Die Formel legt Tobias fest.
5. **Bauen** mit `bookbuild` (Rust, Tobias), Engine-Anbindung (Abschnitt 7),
   Selfplay-Kontrolle in der Variante mit Zufallsauswahl, dann live und
   Lookback mit `tools/book_audit.py` (braucht dafür noch eine
   Varianten-Erweiterung).

## 9. Ergebnis Lauf 30.09./01.10.2026 (Martuni-Analyse + Stockfish-Veto)

Martuni-Analyse 23:21–01:16, Veto (Fairy-Stockfish Tiefe 18, 60 cp)
01:16–02:14. Daten in `book_work/` (nicht versioniert).

| | KotH | Three-check |
|---|---:|---:|
| Stellungen | 75 | 137 |
| davon ≥ 2 Züge nach Veto (buchfähig) | **49** | **53** |
| nur 1 Zug übrig | 25 | 83 |
| Martunis eigener Wurzelzug gestrichen | 19 | 40 |
| SF-Bestzug war unter Martunis Kandidaten | 73 | 133 |

Fast immer war Stockfishs Bestzug schon unter Martunis eigenen
Kandidaten. Das Buch kommt also mit Martunis Zügen aus, Stockfish muss nur
aussortieren.

**Live oft gespielte Züge, die das Veto streicht** (Auswahl, n = Anzahl
Live-Partien, Score aus Martuni-Sicht):

- KotH: 9.…dxe4 nach 1.d4 d5 2.c4 Nf6 3.Nc3 e6 4.Bg5 Bb4 5.e4 (31×,
  10/31 Punkte, −262 cp), danach 11.…Bxc3 (23×, 4/23).
- Three-check: 1.e4 e5 (54×, −172 cp) und 1.Nc3 e5 (33×, −124 cp). Die
  bekannte Falle 1.Nc3 e5 2.e3 Nf6 3.Bc4 Nc6?? (25×, 6/25) ist ein
  erzwungener Verlust (Matt-Score). 2.…Nf6 nach 1.e4 e5 2.Bc4 (14×,
  −639 cp). 6.…Nxf2 / 7.…Bxc3+ in einer Nebenlinie (je 12×, 0 Punkte).

**Engpass ist die Mindestzahl von zwei Zügen**, vor allem in Three-check.
Die Bewertungen schwanken dort stark, und 60 cp sind streng. Buchfähige
Stellungen bei anderer Schwelle:

| Schwelle | KotH | Three-check |
|---:|---:|---:|
| 60 cp | 49 | 53 |
| 100 cp | 59 | 72 |
| 150 cp | 67 | 91 |

Stellungen mit nur einem Zug bleiben laut Tobias-Entscheid draußen, dort
rechnet Martuni selbst. Ausnahmen gibt es nicht. Zur Entscheidung steht:
(a) eine eigene Schwelle je Variante (z. B. Three-check 100 cp), (b) ein
zweiter Analyselauf nur für die Stellungen mit einem Zug, mit mehr
Kandidaten (z. B. Top 8 statt Top 4), oder (c) beides.

**Tobias-Entscheid 01.10.2026:** Veto-Schwelle **Three-check 100 cp,
KotH 60 cp**, dazu ein Nachlauf für alle Stellungen mit weniger als zwei
Zügen: Martuni bewertet dort die besten 8 statt 4 Kandidaten
(`book_analyze.py --refine`), danach ein neues Veto nur für die geänderten
Stellungen (`book_veto.py`, übernimmt vorhandene Urteile mit der neuen
Schwelle).

**Nachlauf 01.10.2026 07:41–09:03** (Top 8, Schwellen 3check 100 / KotH 60 cp):

| | KotH | Three-check |
|---|---:|---:|
| buchfähige Stellungen (≥ 2 Züge) | **56 / 75** | **90 / 137** |
| Buchzüge insgesamt (Ø je Stellung) | 164 (2,9) | 242 (2,7) |
| Martunis eigene Wahl bleibt im Buch | 46 | 82 |

Die übrigen 19 bzw. 47 Stellungen sind über alle Halbzüge verteilt (kein
Cluster). Dort rechnet Martuni selbst. Grundstellung: KotH e4/d4/Nf3/Nc3,
Three-check Nf3/e4/Nc3. Gegen 1.e4 in Three-check bleiben e6/b6/Nc6/Nf6,
1…e5 ist gestrichen. Damit ist die Datenbasis für Gewichtsformel und
`bookbuild` fertig (`book_work/<variante>_analysis.json` + `_veto.json`).

## 10. Gewichtsformel (Tobias-Entscheid 01.10.2026)

Umgesetzt in `tools/book_weights.py` → `book_work/<variante>_book.json`
(Eingabe für `bookbuild`: FEN, Zugfolge ab Grundstellung, Züge mit
Polyglot-Gewicht).

```
roh  = Q_SF · Q_M · L
Q_SF = exp(−SF-Verlust / T)              Abstand zum Stockfish-Bestzug
Q_M  = exp(−min(M-Abstand, 300) / T)     Abstand zu Martunis bestem Buchzug
L    = 2 · (0,5 + (Punkte − Erwartet) / (n + k)),  k = 4,  begrenzt auf [0,05; 2]
T    = Veto-Schwelle (Three-check 100, KotH 60)
Anteil = roh / Summe, jeder Zug mindestens 10 %; Gewicht = Anteil · 1000
```

- **Q_SF**: objektive Qualität. **Q_M**: wie gut Martuni die Folgestellung
  versteht (Stärken). **L**: Live-Bilanz (Schwächen), **gegen die
  Elo-Erwartung** gerechnet, so dass Verluste gegen stärkere Gegner nicht
  unfair bestrafen. Ohne Live-Partien gilt L = 1. Die k = 4 Pseudo-Partien
  auf Erwartungsniveau dämpfen kleine Stichproben.
- Multiplikativ: Ein Zug muss in allen drei Quellen ordentlich sein.
- `book_collect.py` speichert dafür seit 01.10. je Zug die erwarteten Punkte
  (`exp`).

Beispiele (Stand 01.10.):

| Stellung | Gewichte |
|---|---|
| 3check Grundstellung | e4 405, Nc3 369 (89,5/159, erw. 82,9 → L 1,08), Nf3 227 |
| 3check nach 1.e4 | e6 485, Nc6 217 (8/42, erw. 13 → L 0,78), Nf6 193, b6 104 |
| KotH Grundstellung | e4 375, d4 349 (27/41, erw. 22,4 → L 1,21), Nc3 146, Nf3 130 |
| KotH nach 1.d4 | d5 469 (16/46, erw. 22,2 → L 0,75), Nf6 310, e6 222 |

Ergebnis: Three-check 90 Stellungen / 242 Züge, KotH 56 / 164. Neu
hinzugekommene Live-Stellungen (Three-check 5, KotH 2) sind noch nicht
analysiert und bleiben bis zu einem späteren Lauf draußen.
