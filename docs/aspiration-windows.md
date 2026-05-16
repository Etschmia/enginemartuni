# Aspiration Windows — Konzept für Martuni

**Status:** **VERWORFEN nach Smoke-Test 16.05.2026.** Variante B
implementiert und auf den 9 Cluster-1b-Stichproben gemessen — Ergebnis
negativ (−2 Plies Gesamt-Tiefe, Re-Search-Quote 102 %, Schlüsselstellung
W5AboGf0 spielt mit Aspiration den falschen Zug). Code wieder aus
`src/search.rs` entfernt, Doc bleibt als Lerneintrag stehen, Probe-Skript
`tools/probe_aspiration.py` bleibt für künftige Aspiration-Experimente
nutzbar. Befund-Block ganz unten.

**Erwarteter Elo-Gewinn (urspr. Annahme):** +5 bis +15 Elo aus zusätzlicher
Tiefe pro Zeiteinheit, **vorausgesetzt** die Score-Stabilität zwischen
ID-Tiefen genügt dem Startfenster. — Diese Voraussetzung war für Martuni
nicht erfüllt.
**Voraussetzung:** Iteratives Deepening + PVS (beide seit 01.05.2026 in
`search.rs` drin).
**Betroffene Module:** ausschließlich `src/search.rs`, konkret die
ID-Schleife `for depth in 1..=max_depth` (Zeile 280–308). `alpha_beta`
selbst bleibt unverändert.

> **Warum jetzt:** Die Cluster-1b-Diagnose vom 15.05.2026 zeigt für
> `W5AboGf0` und `54iwUiMx`: die Eval ist korrekt, es fehlt **1 Ply
> Tiefe** pro Zeitbudget, damit der Capture nach vorne sortiert. NMP
> und LMR haben die Knotenanzahl pro Zug bereits gedrückt; das nächste
> Stellrad an dieser Stelle ist das Wurzelfenster. Engere Fenster
> sparen Knoten in der Hauptlinie, ohne die Logik der Kindknoten zu
> verändern — direkter Hebel ohne Eval-Risiko.

## Idee in einem Satz

Statt die Wurzel jeder ID-Tiefe mit `(-INF, +INF)` zu suchen, geben
wir ein schmales Fenster um den Score der vorherigen Tiefe vor:
`(last_score - delta, last_score + delta)`. Wenn der wahre Score
innerhalb liegt, sparen wir Knoten (mehr Cutoffs am Wurzelknoten).
Wenn er außerhalb liegt (Fail-High/Low), suchen wir die Tiefe mit
geweitetem Fenster nochmal. Solange Re-Searches selten sind, gewinnen
wir netto Tiefe.

Aspiration ergänzt PVS auf der **Wurzelebene**: PVS schmalt das Fenster
für *Geschwister-Züge* an *jedem* Knoten, Aspiration schmalt es für
den *einen* Wurzelknoten gegenüber dem ganzen Suchbaum. Die Mechanik
ist identisch (Nullfenster-Suche → Re-Search bei Fail), nur der Scope
ist anders.

## Ablauf in der ID-Schleife

```rust
// aktuell (search.rs:280–308)
for depth in 1..=max_depth {
    let score = alpha_beta(&req.board, depth, 0, -INF, INF, 0, ..., &mut state);
    // ...
}

// mit Aspiration
let mut last_score = 0;
for depth in 1..=max_depth {
    let score = if depth < ASPIRATION_MIN_DEPTH || last_score.abs() > MATE_THRESHOLD {
        // Erste paar Iterationen + Mate-Bereich: volles Fenster, kein Risiko
        alpha_beta(&req.board, depth, 0, -INF, INF, 0, ..., &mut state)
    } else {
        aspiration_search(&req.board, depth, last_score, &mut state)
    };
    // ...
    last_score = score;
}
```

`aspiration_search` macht die eigentliche Arbeit:

```rust
fn aspiration_search(board, depth, prev_score, state) -> i32 {
    let mut delta = ASPIRATION_DELTA;          // z.B. 17 cp
    let mut alpha = prev_score - delta;
    let mut beta  = prev_score + delta;

    loop {
        let score = alpha_beta(board, depth, 0, alpha, beta, 0, ..., state);

        if state.stop.load(Ordering::Relaxed) { return score; }

        if score <= alpha {
            // Fail-Low: wahre Score liegt unter unserem Fenster.
            // Beta-Anker behalten (wir wissen, dass score < beta), nur alpha öffnen.
            beta = (alpha + beta) / 2;          // beta runter, näher an alpha (Stockfish-Trick)
            alpha = score - delta;
        } else if score >= beta {
            // Fail-High: wahre Score liegt über unserem Fenster.
            // Alpha-Anker behalten, beta öffnen.
            beta = score + delta;
        } else {
            return score;                       // Treffer im Fenster
        }

        delta = delta + delta / 2;             // exponentielles Widening (Faktor 1.5)
        if delta > ASPIRATION_MAX_DELTA {       // ab hier volles Fenster
            alpha = -INF;
            beta  = INF;
        }

        state.aspiration_researches += 1;       // für Statistik (s. unten)
    }
}
```

Wichtige Eigenschaften:

- **Re-Search-Logik ist symmetrisch zum PVS-Pfad**, den `alpha_beta`
  schon hat. Wir benutzen `alpha_beta` unverändert; nur der äußere
  Loop ruft sie mehrfach pro Tiefe auf.
- **Fail-High öffnet nur β, Fail-Low öffnet nur α.** Die andere Seite
  hat schon einen Cutoff geliefert; wir wissen, der wahre Score liegt
  in dieser Richtung. Das ist deutlich schneller als beidseitig zu
  öffnen.
- **Stop-Signal-Sauberkeit:** Wenn `state.stop` während eines
  Re-Searches feuert, geben wir denselben Score zurück, den die ID-
  Schleife dann nicht mehr verwertet (wie schon heute). Ein Fail-Low
  Score aus einer abgebrochenen Suche darf nicht in `last_score`
  landen — die existierende Stop-Behandlung am Ende der ID-Schleife
  greift hier weiter.

## Parameter — drei sinnvolle Varianten

Reihenfolge: von konservativ nach aggressiv. Tobias entscheidet, ich
mache anschließend nur eine davon.

### Variante A — Stockfish-Default (aggressiv)
- `ASPIRATION_MIN_DEPTH = 5`
- `ASPIRATION_DELTA     = 17` (cp)
- `ASPIRATION_FACTOR    = 1.5` pro Re-Search
- `ASPIRATION_MAX_DELTA = 1000` (danach volles Fenster)

Genau die Werte, die Stockfish heute fährt. Der theoretisch größte
Tiefen-Gewinn, aber auch das höchste Re-Search-Risiko, gerade in
taktisch volatilen Stellungen, die wir häufig haben (Cluster-1b
fluktuiert pro ID-Tiefe um ~50 cp — siehe `trace_w5abogf0`).

### Variante B — moderat (Empfehlung)
- `ASPIRATION_MIN_DEPTH = 5`
- `ASPIRATION_DELTA     = 30` (cp)
- `ASPIRATION_FACTOR    = 2.0` pro Re-Search
- `ASPIRATION_MAX_DELTA = 500`

Doppelt so weites Startfenster wie SF. Begründung: Martuni evaluiert
gerade Material-Hebel feiner als SF (Dynmat) und gleichzeitig Stellung
breiter (Pawn-Struktur weniger fein) — Score-Sprünge pro ID-Iteration
sind in unseren Logs nicht selten >20 cp. Mit 30 cp Startfenster
liegen erwartet ≥80 % der Iterationen im Fenster, und der Faktor 2.0
springt schnell ins volle Fenster bei Volatilität.

### Variante C — defensiv
- `ASPIRATION_MIN_DEPTH = 6`
- `ASPIRATION_DELTA     = 50` (cp)
- `ASPIRATION_FACTOR    = 2.0`
- `ASPIRATION_MAX_DELTA = 400`

Spätester Einstieg, weitestes Fenster — minimaler erwarteter
Tiefen-Gewinn, dafür praktisch kein Risiko, dass häufige Re-Searches
unter dem Strich Tiefe kosten. Sinnvoll, falls A oder B im Smoke-Test
auffallend viele Re-Searches produzieren.

## Mate-Sonderfall

Im aktuellen Code (search.rs:305) bricht die ID-Schleife bei
`score.abs() > MATE_THRESHOLD` ab. Aspiration darf hier nicht
greifen — Mate-Scores sind absolut und passen nicht in ein cp-Fenster
um `last_score`. Implementierung: vor dem `aspiration_search` prüfen
und ggf. volles Fenster fahren (siehe Skizze oben). Außerdem auf
Mate-Treffer **im** `aspiration_search` reagieren: wenn ein Score
zurückkommt, der über `MATE_THRESHOLD` liegt, sofort zurückgeben
(kein weiterer Re-Search nötig).

## Re-Search-Statistik — fürs Lernen und für die Auswertung

Damit wir Cluster-1b-spezifisch sehen können, ob's hilft, loggen wir
pro Suche zwei Zähler:

```rust
struct SearchState {
    // ...
    aspiration_researches: u32,   // Anzahl Re-Searches in dieser Suche
    aspiration_widenings:  u32,   // davon: Anzahl Übergänge ins volle Fenster
}
```

Am Ende der `search`-Funktion als zusätzliche `info string` ausgeben
(taucht in PGN-Annotation und im lichess-bot-Log auf):

```
info string aspiration researches=N widenings=M
```

Das gibt uns nach 50–100 Partien klare Daten für den A/B-Entscheid und
ist gleichzeitig der Hebel, mit dem wir δ und Faktor justieren können,
ohne einen weiteren A/B fahren zu müssen.

## Risiken & Failure Modes

1. **Häufige Re-Searches in volatilen Stellungen → neutraler bis
   negativer Effekt.** Aussortierbar über die Re-Search-Statistik.
   Smoke-Test deckt das auf, bevor wir einen 1000-Partien-Match
   starten.
2. **Repetition/TT-Interaktion.** Aspiration ändert nichts an der
   Repetition-Logik oder der TT-Cutoff-Suppression (siehe
   `feedback_repetition_bug` / `feedback_tt_repetition_poisoning`).
   Die TT-Probe-Logik in `alpha_beta` schaut auf das
   *eingehende* `(alpha, beta)`-Tupel — ein engeres Fenster führt nur
   dazu, dass weniger Knoten den Cutoff bekommen, nie zu mehr falschen
   Cutoffs.
3. **Wechselwirkung mit NMP-Cutoff auf `beta`.** Wenn die ID-Schleife
   ein enges β setzt, feuert NMP an inneren Knoten *früher*. Das ist
   gewünscht (mehr Cutoffs = mehr Tiefe), aber falls NMP an Mate-
   relevante Stellen rauscht, könnte es zur stillen Tiefen-Verkürzung
   führen. Smoke-Test deckt's auf (Stellungen aus
   `probe_cluster1_2026-05-15.txt`, in denen wir das gewünschte
   Verhalten kennen).

## Test-Plan

1. **Smoke-Test (lokal, ~10 min).** Über die 10 Stellungen aus
   `probe_cluster1_2026-05-15.txt` mit `tools/trace_w5abogf0.py`-Logik
   (movetime 60 s, depth ≤ 16) je einmal mit `master` und einmal mit
   `aspiration`-Branch laufen lassen. Erfolgskriterien:
   - Erreichte Maximaltiefe pro Stellung: ≥ +1 Ply im Mittel
   - Re-Search-Quote (`aspiration_researches / depths_searched`): < 30 %
   - Keine Stellung verschlechtert: best move bleibt mindestens so gut
     wie vorher (für die Stellungen mit bekannt richtigem Zug:
     `54iwUiMx` Nxd4, `W5AboGf0` Qxd6 — wenn der bei Tiefe ≤16
     auftaucht, perfekt; wenn nicht, mindestens nicht später als auf
     master)
2. **A/B-Match (fastchess, SPRT [0, 10]).** Standard-Setup:
   `~/tools/fastchess`, UHO_Lichess_4852_v1, 5+0.05, concurrency 2,
   Hash 64MB, 1000 Partien-Cap. Erfolgsgrenzen wie bei den letzten
   A/B-Matches: SPRT akzeptiert H1 ⇒ Rollout; H0 bei klar negativem
   Punktstand ⇒ verwerfen; CI über Null und Lichess-Lift ⇒ wie bei
   Step 2 v2 entscheiden ([[feedback-ab-vs-lichess-signal]]).
3. **Lichess-Lookback.** Bei Rollout: 7-Tage-Snapshot mit
   Blunder-Profil. Erwartung: `missed_capture` (war 0.181/P am 13.05.,
   0.203/P am 15.05.) sinkt um 10–20 %, Rating-Lift +5 bis +15 Elo.
   Wenn Profil flach: Aspiration bringt keine *zusätzliche* Tiefe in
   den taktisch relevanten Stellungen — dann ist Roadmap-Punkt 2
   (MVV-Bonus bei SEE = 0) der direktere Hebel.

## Entscheidung offen

Folgende Punkte will ich von Tobias klären, bevor ich Code schreibe:

1. **Welche Variante (A / B / C)?** Meine Empfehlung ist B; A wäre
   der Stockfish-Originalwert.
2. **Statistik im Output: nur `info string`, oder zusätzlich in eine
   Datei (`logs/aspiration.csv`)?** Letzteres erleichtert die
   Auswertung über 50–100 Partien deutlich, ist aber mehr Code.
3. **Smoke-Test-Tool: separates Skript** (`tools/probe_aspiration.py`,
   das die 10 Stellungen + Statistik abklappert) **oder eine Flag in
   `trace_w5abogf0.py`?** Separates Skript ist sauberer, kostet aber
   eine zusätzliche Datei.

Tobias' Wahl (16.05.2026): Variante B, CSV-Log, separates Skript.

---

## Smoke-Test-Befund (16.05.2026)

Ausgeführt: 9 Cluster-1b-Stichproben aus `tools/probe_missed_captures.py`,
movetime 30 s, max_depth 16, Hash 256 MB, je einmal mit
`MARTUNI_ASPIRATION=off` (Baseline) und einmal `=on` (Variante B,
δ=30 cp, Faktor 2, ab d≥5). Rohlog: `logs/probe_aspiration.out`,
CSV: `logs/aspiration_probe.csv`.

| Stellung | OFF maxD | ON maxD | OFF zug | ON zug | reS (ON) |
|---|---:|---:|:---:|:---:|---:|
| H62xk9vz m14 | 9 | 9 | G | G | 3 |
| zcBr22eo m24 | 9 | 9 | G | G | 7 |
| wo4G1Ae5 m19 | 9 | **10** | ? | **G** | 3 |
| m1oQlfmG m18 | 10 | 9 | **B** | **G** | 8 |
| 54iwUiMx m25 | 8 | 8 | ? | B | 0 |
| 25eZUsMT m19 | **11** | 10 | G | G | 10 |
| **W5AboGf0 m29** | 9 | 9 | **G** | **B** | 2 |
| FLSJc0Sm m12 | 10 | 10 | G | G | 9 |
| wo4G1Ae5 m15 | **9** | 8 | ? | ? | 5 |

**Aggregat:** Σ maxD OFF=84, ON=82 (Δ = −2). Re-Search-Quote
47/46 = **102 %** (Schwelle <30 %), 0 Widenings auf volles Fenster.

**Befund:**
- Aspiration B macht die Suche schlechter, nicht besser:
  Mittelwert maxD −0.22 Ply, Re-Search-Quote >>30 %.
- Cluster-1b-Stellungen sind **Score-Diskontinuitäten** zwischen
  ID-Tiefen — Qxd6 in W5AboGf0 springt von ~−49 cp (d=10 PV-Score)
  auf +47 cp, wenn der Capture endlich vorne sortiert ist. Das sind
  ≈96 cp, weit außerhalb jedes ±30 cp Fensters.
- **W5AboGf0 — die Schlüsselstellung des Plans — wird durch Aspiration
  kaputt gemacht.** OFF findet `Qxd6` (richtig) bei d9, ON spielt `Qg4`
  (falsch) bei gleicher Tiefe. Exakt das Gegenteil dessen, wofür
  Aspiration konzipiert war.
- Konzept-Doc hatte das Risiko als "Failure Mode 1" gelistet
  (häufige Re-Searches → neutral/negativ). Die Daten bestätigen es
  überdeutlich für Martunis aktuelle Eval-Charakteristik.

**Konsequenz:**
- Aspiration-Code aus `src/search.rs` entfernt
  (`git diff --stat src/search.rs` ist nach dem Revert leer).
- Roadmap-Punkt 1 in `docs/roadmap.md` aktualisiert: Aspiration raus,
  Roadmap-Punkt 2 (MVV-Bonus bei SEE=0) rückt vor.
- `tools/probe_aspiration.py` bleibt im Repo — wenn wir später eine
  Aspiration-Variante mit δ≥100 cp testen wollen (z. B. nach Eval-
  Stabilisierungen), ist der Probe-Lauf ein Einzeiler.
- Variante C (defensiv, δ=50 cp) wäre vermutlich auch zu eng — die
  typischen Cluster-1b-Sprünge sind ≥90 cp. δ=100 cp wäre Wettkampf
  mit dem vollen Fenster ohne klaren Gewinn.
