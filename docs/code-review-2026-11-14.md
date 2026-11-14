# Code-Review vom 14.11.2026

Geprüfter Commit: `107ca5557d80c981e879de7064a9846b2b503366`.

## Umfang und Verifikation

Review des aktuellen Repository-Stands mit Schwerpunkt auf UCI-Steuerung,
Suchabbruch, Pondering, Remiserkennung, Transposition Table und
Syzygy-Lebenszyklus. Ergänzend wurden die Brettadapter, Konfiguration,
Buchabfrage und die Import-/Archivierungswerkzeuge betrachtet. Es handelt
sich um eine gezielte Prüfung dieser Abläufe, keine vollständige Prüfung
aller Bewertungsheuristiken und Analysewerkzeuge.

Es wurden keine Quellcodedateien geändert. Die bereits vorhandene Änderung
an `docs/roadmap.md` und die unversionierten Dateien blieben unangetastet.
Builds, Testdaten und Reproduktionsskripte lagen unter `/tmp`; produktive
Konfiguration, Partien, Analysen und Dienste wurden nicht verändert.

Ausgeführt:

```bash
CARGO_TARGET_DIR=/tmp/martuni-review-20261114-target cargo test --offline
CARGO_TARGET_DIR=/tmp/martuni-review-20261114-target cargo build --offline
CARGO_TARGET_DIR=/tmp/martuni-review-20261114-target cargo test --manifest-path tools/bookbuild/Cargo.toml --offline
```

Ergebnis: **223 Engine-Tests und 9 bookbuild-Tests bestanden**, keine
Fehlschläge; ein Engine-Benchmark war als `ignored` markiert. Beim
Engine-Testbuild wurde ein unbenutzter `MoveGen`-Import in `src/syzygy.rs:374`
gemeldet. Dieser Hinweis wird nicht als funktionaler Befund gezählt.
Die Matt- und Three-Check-Grenzstellungen wurden zusätzlich mit dem lokal
installierten `python-chess` unabhängig auf ihr Spielende geprüft.

Die UCI-Reproduktionen verwendeten die separat gebaute Debug-Binary,
`Hash=1 MB`, deaktivierte Syzygy-Tabellen und eine eigene Konfiguration ohne
Bücher; nur der explizite Buchtest verwendete ein synthetisches Buch mit
einem einzigen legalen Startstellungszug. Gemessene Zeiten illustrieren das
Fehlverhalten auf diesem Rechner und sind keine Leistungsbenchmarks.

## Befunde

**10 Befunde: vier P1 und sechs P2.** P1 bezeichnet erhebliche
Steuerungsprobleme oder Risiken für Analyseergebnisse; P2 bezeichnet
funktionale Fehler in bestimmten Such-, Zeit- oder Konfigurationssituationen.
Neun Befunde wurden lokal reproduziert. Befund 10 ist statisch anhand des
Aufrufpfads und der tatsächlich eingebundenen Dependency bestätigt.

### 1. [P1] `isready` blockiert während einer Suche auch `stop` und `quit`

**Fundstelle:** `src/uci.rs:67–71`.

Der UCI-Thread ruft bei `isready` zuerst `join()` auf dem Suchthread auf.
Eine laufende Suche muss diesen Bereitschaftstest jedoch beantworten können.
Solange `join()` wartet, liest der einzige UCI-Eingabethread keine weiteren
Kommandos. Ein danach gesendetes `stop` oder `quit` kann das Stop-Flag deshalb
nicht setzen. Bei einer langen Suche wirkt die Engine dadurch festgefahren.

**Reproduktion:** Ohne Buch `position startpos`, dann `go depth 50` senden.
Nach ungefähr 200 ms `isready` senden, weitere 100 ms später `stop`. In den
folgenden 700 ms erschienen weder `readyok` noch `bestmove`; die Suche lief
weiter. Der isolierte Testprozess musste beendet werden.

**Korrekturrichtung:** `isready` unabhängig vom Ende einer aktiven Suche
beantworten und den UCI-Eingabethread für Abbruchkommandos verfügbar halten.
Ein Regressionstest sollte `isready` und anschließend `stop` während einer
laufenden Suche senden und beide Antworten mit einem Timeout prüfen.

### 2. [P1] Ponder-Suchen veröffentlichen Ergebnisse vor `ponderhit` oder `stop`

**Fundstellen:** `src/search.rs:337–345`, `src/search.rs:436–485`,
`src/uci.rs:275–288`.

Ein Buchtreffer kehrt auch im Ponder-Modus sofort aus `search()` zurück.
Ebenso beendet ein gefundenes Matt die Iterationen, und ein explizites
Tiefenlimit lässt die Schleife auslaufen. `spawn_search()` gibt das Ergebnis
anschließend ohne Prüfung des Ponder-Zustands als `bestmove` aus. Damit wird
ein Zug veröffentlicht, während die Engine noch auf der Gegnerzeit rechnen
soll. Die besondere Behandlung des forcierten Einzelzugs schützt diese
anderen Rückkehrpfade nicht.

**Reproduktionen ohne vorheriges `ponderhit` oder `stop`:**

- Startstellung ohne Buch, `go ponder depth 1`: `bestmove e2e4` nach etwa 1 ms.
- FEN `7k/8/5KQ1/8/8/8/8/8 w - - 0 1`,
  `go ponder wtime 60000 btime 60000`: `score mate 1` und
  `bestmove g6g7` nach etwa 18 ms.
- Startstellung mit synthetischem Buchzug `e2e4`,
  `go ponder wtime 60000 btime 60000`: sofort `book hit` und `bestmove e2e4`.

**Korrekturrichtung:** Ein fertig berechnetes Ponder-Ergebnis zurückhalten,
bis `ponderhit` oder `stop` empfangen wird. Diese Freigabe muss alle
Ergebnispfade einschließlich Buch, Matt und Tiefenlimit abdecken.

### 3. [P1] Schleusen-Import bestätigt Ergebnisse vor ihrer dauerhaften Speicherung

**Fundstelle:** `tools/schleuse_sync.py:225–233`.

`import_results()` verschiebt jede erfolgreich gelesene Ergebnisdatei nach
`done/` und entfernt die Outbox-Kopie, obwohl der zusammengeführte Zustand
bisher nur im RAM liegt. Erst nach Verarbeitung des gesamten Stapels folgt
`save_state()`. Ein Schreibfehler, Prozessabbruch oder eine Exception bei
einem späteren Ergebnis hinterlässt somit bereits bestätigte Ergebnisse,
die in der Analysedatei fehlen. Der nächste Lauf importiert `done/` nicht.
Die atomare Ersetzung innerhalb von `save_state()` schützt diese Reihenfolge
nicht.

**Reproduktion:** In temporären Verzeichnissen eine gültige Ergebnisdatei
`inbox/a.pgn.json` und `outbox/a.pgn` angelegt. Nur für den Test wurde
`save_state()` per Mock durch einen `OSError` ersetzt. Nach dem Fehler:

```text
inbox/a.pgn.json: fehlt
done/a.pgn.json: vorhanden
outbox/a.pgn: fehlt
Analysedatei: fehlt
erneuter import_results()-Aufruf: (0, 0)
```

Die Rohdaten bleiben zwar in `done/` manuell wiederherstellbar; im
automatischen Importablauf sind sie verloren. Eine erneute Analyse der PGN
kann den Verlust später verdecken, verursacht aber zusätzliche Arbeit.

**Korrekturrichtung:** Zuerst den zusammengeführten Zustand dauerhaft
speichern, erst danach die zugehörigen Inbox-Dateien bestätigen/verschieben
und Outbox-Dateien entfernen. Die vorhandene Deduplizierung eignet sich für
einen erneuten Import nach einem Abbruch zwischen Speicherung und Bestätigung.

### 4. [P1] Archivierung und Schleusen-Sync verwenden unterschiedliche Locks

**Fundstellen:** `tools/archive_after_rollout.py:196–203`,
`tools/analyze_cron.py:68`, `tools/schleuse_sync.py:67`,
`tools/schleuse_sync.py:283–286`.

Die Archivierung ruft `ac.acquire_lock()` mit dessen Standardpfad
`analyze_cron.pid` auf. Der aktive Schleusen-Sync setzt dagegen vor seinem
Lock-Aufruf `ac.LOCK_PATH` auf `schleuse_sync.pid`. Da die Skripte in
getrennten Prozessen laufen, schützt der Archivierungs-Lock nicht vor einem
gleichzeitigen Sync-Tick.

Ein Sync kann dadurch den alten Analysezustand laden, während die
Archivierung dieselbe Datei verschiebt und die Konfiguration umstellt.
Schreibt der Sync anschließend auf den alten Pfad zurück, entsteht dort
erneut eine Datei; das bereits archivierte Exemplar enthält die neuen
Importe nicht. Auch das gemeinsame Verschieben von PGNs und `done/` ist
dadurch nicht geschützt.

**Nachweis:** Mit den von beiden Skripten verwendeten Lock-Dateinamen in
einem temporären Verzeichnis konnten beide `ac.acquire_lock()`-Aufrufe
erfolgreich sein, während beide Lockdateien gleichzeitig existierten.
Die Datenkorruption selbst wurde nicht an produktiven Dateien ausgelöst.

**Korrekturrichtung:** Archivierung und Sync müssen denselben,
prozessübergreifend exklusiven Lock für gemeinsame Daten verwenden. Ein
Test sollte nach gehaltenem Sync-Lock den Archivierungszugriff ablehnen
und umgekehrt.

### 5. [P2] Die 50-Züge-Prüfung verdeckt einen unmittelbar ausgeführten Mattzug

**Fundstelle:** `src/search.rs:585–592`; die Terminal-Erkennung erfolgt
erst später beziehungsweise in `quiescence()`.

`alpha_beta()` gibt an inneren Knoten bei `halfmove >= 100` sofort Remis
zurück, bevor geprüft wird, ob der letzte Zug mattgesetzt hat. Ein Mattzug
beendet die Partie jedoch unmittelbar. Bei Halbzugzähler 99 kann die Engine
deshalb einen vorhandenen Mattzug verwerfen und stattdessen Remis spielen.

**Reproduktion ohne Buch und Syzygy, jeweils frischer Prozess:**

```text
position fen 7k/8/5KQ1/8/8/8/8/8 w - - 0 1
go depth 1
=> score mate 1 ... pv g6g7
=> bestmove g6g7

position fen 7k/8/5KQ1/8/8/8/8/8 w - - 99 1
go depth 1
=> score cp 0 ... pv g6b1
=> bestmove g6b1
```

`Qg7#` ist in beiden Stellungen möglich; nur der Halbzugzähler unterscheidet
sich. **Korrekturrichtung:** Unmittelbar entscheidende Endstellungen vor
dem Remis-Cutoff erkennen. Dabei auch unmittelbare Varianten-Siege beachten.

### 6. [P2] TT-Scores werden trotz unterschiedlicher Halbzugzähler übernommen

**Fundstelle:** `src/search.rs:650–666`; TT-Schlüssel aus
`board.get_hash()` in `src/search.rs:583`.

Der Stellungs-Hash enthält den Halbzugzähler nicht. Ein TT-Score-Cutoff
prüft zwar den Wiederholungskontext, aber nicht die 50-Züge-Situation des
gespeicherten Ergebnisses. Dadurch wird ein mit frischem Zähler berechneter
Gewinnscore kurz vor der Remisgrenze weiterverwendet, ohne die jetzt
erzwungenen Remisblätter zu besuchen.

**Reproduktion ohne Buch und Syzygy:** Zuerst im selben Prozess
`position fen 7k/8/8/8/8/8/8/R3K3 w - - 0 1` mit `go depth 2` suchen.
Danach dieselbe Stellung mit Zähler 98 setzen und erneut `go depth 2`
ausführen. Vergleich mit einem frischen Prozess:

| Suche | Score bei Tiefe 2 | Knoten | Bestmove |
|---|---:|---:|---|
| Zähler 0, Aufwärmsuche | +640 cp | 11465 | `e1d2` |
| Zähler 98, vorhandene TT | +640 cp | 32 | `e1d2` |
| Zähler 98, frische TT | 0 cp | 66 | `a1b1` |

In dieser Stellung kann innerhalb der verbleibenden Halbzüge weder
mattgesetzt noch ein Bauern-/Schlagzug ausgeführt werden. Der warme
TT-Score ist daher falsch. **Korrekturrichtung:** Die Gültigkeit von
TT-Scores an den Halbzugzähler binden, etwa über zusätzliche Metadaten oder
einen separaten TT-Schlüssel; den Wiederholungs-Hash dabei unverändert
lassen. Ein Move-Hinweis kann auch bei ungültigem Score nutzbar bleiben.

### 7. [P2] Three-Check-FENs setzen den Such-Halbzugzähler immer auf null

**Fundstelle:** `src/position.rs:48–52`.

`set_fen()` liest den Halbzugzähler stets aus dem fünften FEN-Feld
(`nth(4)`). Bei den unterstützten Three-Check-FENs steht dort jedoch der
Schachzähler, beispielsweise `3+3`; der eigentliche Halbzugzähler folgt
danach. Das Parsen als `u8` schlägt fehl und `unwrap_or(0)` setzt die Uhr
stillschweigend zurück. Das Brett enthält weiterhin die korrekten
Varianteninformationen, während die Suche mit einem falschen Zähler läuft.

**Reproduktion:** Nach `setoption name UCI_Variant value 3check` jeweils
in einem frischen Prozess die folgende Stellung mit `go depth 1` suchen:

```text
7k/8/8/8/8/8/8/R3K3 w - - 3+3 0 1
7k/8/8/8/8/8/8/R3K3 w - - 3+3 99 1
```

Beide Läufe liefern identisch `score mate 3`, `pv a1a8` und 2421 Knoten.
Im zweiten Fall erreicht bereits der erste reversible Zug die Remisgrenze;
er ist weder Matt noch das dritte Schach. Der Zähler 99 muss daher einen
anderen Suchverlauf bewirken.

**Korrekturrichtung:** Den Zähler aus dem Backend beziehungsweise dem bereits
geparsten FEN-Zustand beziehen, statt für alle Varianten eine feste
Feldposition anzunehmen. Varianten-FENs mit nichtnull Zählern testen.

### 8. [P2] `go infinite` wird nach ungefähr einer Sekunde beendet

**Fundstellen:** `src/uci.rs:359–395`, `src/search.rs:2198–2212`.

Der Parser ignoriert `infinite`; `GoParams` speichert diesen Modus nicht.
Ohne Uhr und ohne explizite Tiefe setzt die Zeitberechnung anschließend
30 Sekunden Restzeit an und teilt diese durch 30. Dadurch bekommt eine
angeforderte Dauersuche dieselbe Deadline wie ein einfaches `go`.

**Reproduktion:** Startstellung ohne Buch, `go infinite`, anschließend
keinerlei weitere Eingaben. Die Engine gibt nach ungefähr 0,998 Sekunden
`bestmove e2e4 ponder d7d5` aus.

**Korrekturrichtung:** `infinite` explizit parsen und als Suche ohne
Zeitdeadline behandeln. Das Ergebnis darf erst auf `stop` veröffentlicht
werden; dabei auch früh fertige Ergebnisse wie Buchtreffer berücksichtigen.

### 9. [P2] Mindestbudget von 50 ms überschreitet kleine Restzeiten

**Fundstelle:** `src/search.rs:2209–2212`.

Sowohl das gewünschte Budget als auch dessen Obergrenze werden auf
mindestens 50 ms angehoben. Bei weniger als 50 ms Restzeit liegt damit
selbst die vermeintliche Obergrenze über der gesamten verfügbaren Zeit.
`MoveOverhead` kann diese Überschreitung nicht verhindern. Zusätzlich wird
die Deadline nur periodisch geprüft, was die reale Antwort weiter verzögert.

**Reproduktion:** Startstellung ohne Buch, `go wtime 1 btime 1`.
`bestmove` erschien nach ungefähr 75 ms, obwohl nur 1 ms Restzeit gemeldet
wurde. Das aus den Formeln abgeleitete Budget beträgt hier bereits 50 ms.
Damit drohen vermeidbare Zeitüberschreitungen in Zeitnot.

**Korrekturrichtung:** Die tatsächliche Restzeit abzüglich Sicherheitsreserve
muss die harte Obergrenze bilden. Ein normales Mindestbudget darf diese
Grenze nicht anheben; bei extrem knappen Uhren früher abbrechen oder den
legalen Fallback-Zug ausgeben.

### 10. [P2] Wechsel von `SyzygyPath` scheitert am noch lebenden alten Handle

**Fundstellen:** `src/uci.rs:97–99`, `src/syzygy.rs:123`;
Dependency `pyrrhic-rs 0.2.0`, `src/tablebases.rs:114–117` und `:242–247`.

Beim Pfadwechsel wird zuerst die rechte Seite von
`syzygy = load_syzygy(&options.syzygy_path)` ausgewertet. Das alte
`Option<Arc<Syzygy>>` lebt zu diesem Zeitpunkt noch. `load_syzygy()` versucht
über `TableBases::new()` eine zweite Initialisierung. Die tatsächlich
eingebundene Version von `pyrrhic-rs` weist diese ausdrücklich mit
`AlreadyInitialized` zurück, solange das alte Handle nicht freigegeben ist.
Der Wrapper macht daraus `None`, das anschließend das alte Handle ersetzt.

**Auswirkung:** Nach erfolgreich geladenem Pfad A schaltet ein direkter
Wechsel auf einen anderen gültigen Pfad B die Tabellen ab. Gleichzeitig
steht in `options.syzygy_path` bereits B. Dasselbe `setoption` für B erneut
zu senden hilft nicht, weil dann kein Pfadwechsel mehr erkannt wird.

**Nachweisgrenze:** Statisch anhand der Auswertungsreihenfolge und des lokal
vorliegenden Dependency-Quellcodes bestätigt. Kein Lauf mit echten
Tablebase-Dateien durchgeführt.

**Korrekturrichtung:** Vor einer Neuinitialisierung laufende Benutzer des
alten Handles koordinieren und alle alten Handles freigeben. Den gesetzten
Optionswert und den tatsächlichen Ladezustand konsistent halten; A→B und
einen erneuten Ladeversuch nach einem Fehler testen.

## Nachverfolgung (14.11.2026, Claude)

Alle zehn Befunde wurden am Code nachgeprüft: **alle zehn sind stichhaltig**
und behoben. Jeder Fix hat einen eigenen Commit und einen Regressionstest.
Stand danach: 230 Engine-Tests, 8 UCI-Integrationstests
(`tests/uci_protocol.rs`, neu — starten die Binary mit isolierter
Konfiguration) und 5 Python-Tests (`tools/test_schleuse_tools.py`, neu)
bestanden.

| # | Befund | Urteil | Commit | Wirkt auf Spielstärke? |
|---|---|---|---|---|
| 1 | `isready` blockiert `stop`/`quit` | stichhaltig | `6fbe7ec` | nein |
| 2 | Ponder-Ergebnis vor `ponderhit`/`stop` | stichhaltig | `dc1554f` | nein (nur Ponder-Protokoll) |
| 3 | Schleusen-Import bestätigt vor dem Speichern | stichhaltig | `c4fa077` | nein (Werkzeug) |
| 4 | Archivierung und Sync mit verschiedenen Locks | stichhaltig | `4d2c880` | nein (Werkzeug) |
| 5 | 50-Züge-Prüfung verdeckt Matt | stichhaltig | `8c84e7d` | ja, sehr selten |
| 6 | TT-Scores trotz anderem Halbzugzähler | stichhaltig | `6863a02` | ja, ab Zähler 80 |
| 7 | Three-Check-FEN: Halbzugzähler immer 0 | stichhaltig | `30ac379` | nur Three-Check |
| 8 | `go infinite` endet nach 1 s | stichhaltig | `556b17e` | nein |
| 9 | Mindestbudget 50 ms über der Restzeit | stichhaltig | `4337d50` | ja, unter 100 ms Restzeit |
| 10 | `SyzygyPath`-Wechsel schaltet Tabellen ab | stichhaltig | `84842cc` | nein |

### Was jeweils geändert wurde

1. **`isready`:** antwortet sofort, ohne `join()` auf den Such-Thread.
   Test: `isready` und danach `stop` während `go depth 50`.
2. **Ponder:** Der Such-Thread hält ein fertiges Ergebnis zurück, bis
   `ponderhit` das Ponder-Flag löscht oder `stop`/`quit`/EOF das Stop-Flag
   setzt. Das deckt Buchtreffer, Matt und Tiefenlimit gemeinsam ab, weil die
   Sperre hinter `search()` sitzt. Zusatz: Kommt während einer wartenden
   Ponder-Suche ein neues `go` (Protokollverstoß), wird sie abgebrochen,
   statt den UCI-Thread im `join()` aufzuhängen. Tests für Tiefenlimit und
   Matt; der Buchpfad läuft über dieselbe Sperre, hat aber keinen eigenen
   Test (bräuchte ein synthetisches Buch).
3. **Schleusen-Import:** je Zustandsdatei erst `save_state()`, dann die
   zugehörigen Ergebnisse nach `done/` verschieben und die Outbox-Kopien
   löschen. Bricht der Lauf dazwischen ab, liegt das Ergebnis noch in
   `inbox/` und wird beim nächsten Tick erneut importiert; `merge_result`
   dedupliziert. Tests für beide Abbruchstellen. **Nicht geändert:** Ein
   `.failed.json` wird weiterhin vor `save_quarantine()` verschoben. Bei
   einem Abbruch dort geht höchstens ein Fehlschlag-Zähler verloren, die
   PGN wird neu angeboten.
4. **Locks:** `archive_after_rollout.py` nimmt jetzt `schleuse_sync.pid`.
   Zusätzlich legt `acquire_lock()` die Lock-Datei atomar an (`O_EXCL`);
   vorher lagen Prüfen und Schreiben getrennt, zwei gleichzeitig startende
   Prozesse konnten beide durchkommen. Tests: gleicher Lock-Pfad, lebender
   Fremd-Lock wird abgelehnt, verwaister Lock wird ersetzt. Der Sync-Cron
   läuft seit dem Commit mit dem neuen Code; ein Probelauf (`--dry-run`)
   war unauffällig.
5. **Matt vor Remis:** Bei Zähler ≥ 100 prüft `alpha_beta()` zuerst, ob die
   Seite am Zug keine legalen Züge hat, und bewertet dann über
   `terminal_score()` (Matt, Varianten-Ende; Patt bleibt 0). Die
   Zuggenerierung fällt nur an diesen Knoten an. Test: Reproduktion aus dem
   Befund (`Qg7#` bei Zähler 99).
6. **TT und Halbzugzähler:** Ab Zähler 80 mischt `tt_key()` den Zähler in
   den TT-Schlüssel; dieselbe Stellung mit Zähler 85 und 98 hat dann
   getrennte Einträge. Darunter bleibt der Schlüssel der reine Hash, die
   Suche dort ist unverändert (Stichprobe: drei Stellungen, Tiefe 9,
   Knotenzahl identisch zur Basis). Die Wiederholungserkennung nutzt weiter
   den reinen Hash, der Pondermove-Lookup den neuen Schlüssel. Test:
   Reproduktion aus dem Befund (K+T gegen K, Zähler 0 dann 98, gleiche TT →
   jetzt 0 wie mit frischer TT).
   **Verworfene Alternativen:** (a) Zähler immer mischen — verliert im
   Mittelspiel Zugumstellungen, bei denen ein Bauernzug an anderer Stelle
   steht; (b) TT-Cutoffs bei hohem Zähler ganz abschalten — einfacher, kostet
   aber gerade in langen Endspielen Suchtiefe. **Restrisiko:** Ein unter
   Zähler 80 abgelegter Eintrag mit mehr als 20 Halbzügen Suchtiefe kann
   ein 50-Züge-Blatt enthalten und bei leicht anderem Zähler wiederverwendet
   werden. Die Schwelle 80 ist ein Schätzwert, nicht gemessen.
7. **Three-Check-FEN:** Der Halbzugzähler ist jetzt das erste rein
   numerische Feld ab Position fünf. Das passt für Standard-FENs, das
   Lichess-Format (`3+3 99 1`) und das ältere Format (`99 1 +0+0`). Tests
   für alle drei und für `Position<BoardThreeCheck>`.
8. **`go infinite`:** wird geparst (`GoParams.infinite`), hat keine
   Deadline, und das Ergebnis wird bis `stop` gehalten — auch ein sofort
   fertiges (Matt, Buch). Tests für beides.
9. **Zeitbudget:** Der Deckel „Restzeit minus 50 ms“ ist jetzt hart
   (mindestens 1 ms) und wird nicht mehr auf 50 ms angehoben. Ab 100 ms
   Restzeit ist das Budget unverändert. Bei 1 ms Restzeit ohne Buch kommt
   `bestmove` jetzt nach etwa 1 ms (Tiefe 2 fertig) statt nach 75 ms.
   `MoveOverhead` wird beim Deckel weiterhin nicht abgezogen; das wäre ein
   eigener Schritt.
10. **`SyzygyPath`:** Das alte Handle wird vor dem Neuladen freigegeben
    (`Arc::try_unwrap`). Hält ein laufender Such-Thread noch einen Klon,
    wird der Wechsel abgelehnt und der alte Optionswert wiederhergestellt.
    Ein erneutes `setoption` nach einem Ladefehler löst einen neuen
    Ladeversuch aus. Test mit den echten Tabellen aus `~/syzygy/3-4-5`
    (A → Symlink B, danach `syzygy root hit`); ohne Tabellen wird er
    übersprungen. Damit ist der Befund auch im Lauf bestätigt, nicht nur
    statisch.
    **Nebenbefund, mitbehoben:** Ein Pfad ohne Tabellendateien wurde als
    „tablebases loaded (up to 7 men)“ gemeldet, weil pyrrhic dort 7
    zurückgibt. `Syzygy::load` liefert jetzt `None`, wenn keine
    Tabellendatei gefunden wird.

Der gemeldete unbenutzte `MoveGen`-Import in `src/syzygy.rs` ist mit Commit
`84842cc` entfernt.

### Offen

- Kein Rollout: Die Commits liegen lokal auf `master`, nicht gepusht, Horst
  nicht informiert.
- Wettkampf alte gegen neue Engine (15 Partien, 3+0, Standard) wegen der
  Befunde 5, 6 und 9: Ergebnis siehe unten.
