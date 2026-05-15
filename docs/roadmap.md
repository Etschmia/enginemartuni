# Martuni — Roadmap

Zentrale Übersicht der nächsten Schritte und der bisherigen Maßnahmen.
Detail-Begründungen, Mess-Verläufe und Konzepte stehen in den verlinkten
Einzeldokumenten:

- Search: [lmr-plan.md](lmr-plan.md), [null-move-pruning.md](null-move-pruning.md), [see.md](see.md)
- Evaluation: [eval-kalibrierung.md](eval-kalibrierung.md), [endgame.md](endgame.md), [vorbereiteter_Prompt_dynamische_Figurenbewertung.md](vorbereiteter_Prompt_dynamische_Figurenbewertung.md)
- Tooling: [blunder-analyse.md](blunder-analyse.md)

## Aktueller Status

**Dynmat-Step2 v2 am 15.05.2026 nach 144-Partien-Lichess-Lookback
behalten.** Lichess-Rating 15.05. 15:00: Blitz **2010** (−17 ggü 12.05),
Schnellschach **2083** (+8) — netto flach innerhalb Rauschen, kein
Rollback-Trigger erfüllt. Decisive-Blunder-Rate stabil; `allows_mate`-
Spike (0.080 → 0.188/P) ist Statistik-Artefakt: alle 27 Fälle aus bereits
verlorenen Stellungen (`eval_before < −300cp`), 7 davon stammen aus
einem einzigen Endspiel-Shuffling-Spiel (`pD9ZAV3G` vs stickshark99).
`missed_mate` bleibt mit 0.035/P stabil niedrig.

Vorgeschichte: Step 2 v2 wurde am 13.05. **trotz nicht-signifikantem
A/B** ausgerollt (Step2v2 +5.56 Elo ±18.89 vs Step1, LOS ~72 %).
Dieselbe Datenlage hatte Step 1 schon im SPRT formal nicht bestanden,
auf Lichess aber +77/+20 geliefert. fastchess-Selfplay testet nur den
Spiegelstil; Lichess deckt ein breiteres Stilspektrum ab und ist für
Pawn-/Material-Hebel empirisch der bessere Indikator.

**Befunde aus dem 15.05-Lookback und Cluster-1-Stichprobe:**
- Vier Buch-Patches für `stickshark99` ausgerollt (15.05.,
  `martuni_patches.bin` jetzt 6 Einträge).
- Cluster-1-Stichprobe `missed_capture` (`tools/probe_missed_captures.py`):
  3/9 lösen sich mit Tiefe (Such-/NPS-Hebel), 3/9 sind echte Eval-Hebel
  in der Erstbewertung — nach SF-Sanity-Check bis d30
  (`tools/verify_cluster1b_stockfish.py`) bleiben **2/9 als echte
  Bugs**: `54iwUiMx` m25 (Nxd4) und `W5AboGf0` m29 (Qxd6).
  `m1oQlfmG` war SF-Artefakt der d17-Momentaufnahme (Stockfish d30
  selbst flippt auf `Be2`).
- **W5AboGf0 Deep-Dive** (`tools/trace_w5abogf0.py`): Engine bei
  120 s/100 M Knoten erreicht d10 und bleibt bei Qg4 (−49 cp).
  Post-Move-Eval an d10 separat: Qxd6 = **+47 cp**, Qg4 = **−44 cp**
  (Diff 91 cp). SEE rechnet die Q-für-Q-Capture korrekt mit Netto 0.
  Diagnose: **Such-Tiefe-/Move-Ordering-Issue, kein statischer
  Eval-Bug**. Bei Root-d11 wäre der Flip da, kostet aber ~5–10× mehr
  Knoten als das Blitz-Budget liefert.
- Neue Hotspots: `stickshark99` 2.25 B/P (vor den Patches),
  `AetherBot` 2.04 B/P (25 Spiele) — eigener Lookback offen;
  `sxphia` von 1.7 auf 1.42 B/P gefallen.

## Nächste Schritte

1. **Aspiration Windows** — **hochgezogen aus den Offenen Themen.**
   Cluster-1b zeigt: Eval ist korrekt, es fehlt 1 Ply Tiefe pro
   Zeiteinheit. Engeres Startfenster pro ID-Tiefe spart Knoten bei
   stabiler Bewertung, genau für solche Stellungen der direkte Hebel.
   Erwartung: +1 Ply in vergleichbarer Zeit, Cluster-1b-Stellungen
   flippen dann auf den richtigen Capture-Zug. Falls Aspiration
   Re-Searches häufiger auslösen als erwartet, schlimmstenfalls neutral.
2. **Move-Ordering: MVV-Bonus auch bei SEE = 0 Captures mit
   high-value-victim**. Qxd6 hat SEE 0 (Q für Q) und sortiert sich
   aktuell wie ein "neutraler Schlag". Ein zusätzlicher MVV-Bias würde
   Damen-/Turm-Captures vor neutrale Bauern-Captures heben — ohne
   Tiefen-Gewinn könnte das in `W5AboGf0` reichen, Qxd6 als ersten
   Capture-Versuch an der Wurzel zu wählen. Klein, lokal, risikoarm.
3. **`connected_rooks_pair = 30` ausgerollt am 15.05.2026.** A/B-Match
   `matches/conn_rooks_150_vs_30` lief 15.05. 17:15–17:39, SPRT [0, 10]
   nach 240 Partien terminiert (H0 akzeptiert): **CR30 schlägt CR150
   um +150.65 Elo ±41.14**, LOS 100 % für CR30, Ptnml [45, 22, 43, 6, 4],
   PairsRatio 0.15, Total Time 00:23:54. Damit ist der Eval-Audit-
   Befund (92–99 % Bias-Treiber) live bestätigt — selfplay-Signal so
   deutlich, dass das übliche A/B-↔-Lichess-Caveat nicht greift.
   `eval.toml` im Repo-Root auf 30 gesetzt; kein Rebuild nötig
   (Laufzeit-Config). Lichess-Lookback am 16.05. Bei Plateau evtl.
   Folge-A/B 30 vs 0 oder 30 vs 60.
4. **Dynamische Figurenbewertung Schritt 3** — dynamisches Bishop-Pair
   (Phase + Brett-Offenheit). **Zurückgestellt** hinter (1)–(3):
   Cluster-1b-Befund zeigt, dass aktuelle Search-Tiefe der größere
   Engpass ist als feinere Eval-Hebel.
5. **NMP-Verfeinerungen** (adaptive R, Verification Search) — erst wenn
   die Endgame-Rate Anlass gibt; aktuell kein Druck.

## Offene Themen — Search

- **Futility / Reverse Futility Pruning** — Blattnähe-Pruning, wenn die
  statische Bewertung selbst mit großzügigem Margin Alpha nicht erreicht.
- **Lazy MovePicker** — inkrementelle Zuggenerierung (Hash → Captures →
  Killer → Quiet) statt vorab vollständig sortierten Vektor; spart
  Rechenzeit bei frühen Cutoffs.
- **LMR Variante B** — logarithmische Reduktionsformel mit Lookup-Table
  als A/B-Test gegen Variante A (siehe [lmr-plan.md](lmr-plan.md)).
- **LMR auch in PV-Knoten** — Stockfish-Stil mit konservativeren
  Reduktionswerten (siehe [lmr-plan.md](lmr-plan.md)).

## Offene Themen — Evaluation

- **Backward Pawns** — Strafe für Bauern ohne Nachbarbauern hinter sich,
  deren Vorrückfeld vom Gegner sicher kontrolliert wird.
- **Outposts (Springer)** — Bonus für Springer auf gedeckten Zentralfeldern,
  die durch gegnerische Bauern nicht mehr vertrieben werden können (siehe
  [see.md](see.md), Abschnitt „Offene Schritte").
- **Dynamischer Bishop-Pair-Bonus** — Bonus skaliert mit Brett-Offenheit
  (umgekehrt proportional zur Bauernanzahl). Aktuell statischer Fixwert
  `bishop_pair_each`.
- **Pawn-Endgame-Guard** — Opposition in K+P-vs-K plus Square-of-the-Pawn,
  als ergänzendes Wissen zum bereits vorhandenen `kpk_score` in
  [endgame.rs](../src/endgame.rs).
- **Tapering für Passbauern und isolierte Bauern** — Passbauer-Bonus
  per Rang ist da (`pawn_passed_rank_bonuses`), expliziter MG/EG-Split
  fehlt; isolierte Bauern sind phasenflach mit −20 cp (siehe
  [eval-kalibrierung.md](eval-kalibrierung.md), Punkte 2 und 3).
- **Springer- vs. Läufer-Differenzierung** — N=B=300 cp ist nicht
  stellungsabhängig; Plan in
  [vorbereiteter_Prompt_dynamische_Figurenbewertung.md](vorbereiteter_Prompt_dynamische_Figurenbewertung.md).
- **Bishop-Trap-Detection** (siehe [see.md](see.md)).
- **Pawn-Shield bei nach vorne gegangenem König** — kleine Schwäche, in
  [eval-kalibrierung.md](eval-kalibrierung.md) notiert.

## Offene Themen — Performance / Code-Qualität

- **SEE inkrementell** — `all_attackers_to` cachen statt pro Schlag neu
  berechnen (siehe [see.md](see.md)).
- **`Score`-Struct für Tapered Eval** — `(i32, i32)`-Tupel ablösen;
  `Add`/`Sub`/`Mul`-Traits, finale Interpolation per `score.taper(phase)`.
- **Iterative Deepening auslagern** — eigener `SearchState`-Methode, damit
  `search()` schlanker wird.
- **Benannte Ordering-Konstanten** — magische Zahlen in
  `order_moves` durch benannte Konstanten oder ein Stage-Enum ersetzen.
- **`is_passed_simple` (search.rs) vs. `is_passed` (eval.rs)** sind nach
  dem 09.05-Bitmask-Refactor byte-identisch. Bei Gelegenheit
  konsolidieren (eine Instanz, gemeinsamer Aufrufer).

## Verlauf

*Chronologische Zusammenfassung der bereits umgesetzten Maßnahmen.
Details und Mess-Daten in den verlinkten Dokumenten.*

- **Debug-Eval-Breakdown + `connected_rooks_pair`-Befund (15.05.2026) — DONE.**
  Neues UCI-Kommando `eval` druckt jede Eval-Komponente als
  `info string`-Zeile (`src/eval.rs` `evaluate_breakdown`,
  `print_eval_breakdown`; rein additiv, alle Tests grün). Damit
  Validierungs-Tool `tools/validate_connected_rooks.py` über
  mehrere Stellungen gefahren. Befund: `connected_rooks_pair = 150`
  erklärt **92–99 %** des Eval-Bias in beiden echten Cluster-1b-
  Stellungen — `W5AboGf0` post Qxd6 Nxd6: Bias +161 cp, davon
  +150 vom Term; `54iwUiMx` m25 vor Ne7: Bias −162 cp, davon −150
  vom Term. Nach Abzug Rest-Gap ±11–12 cp (Rauschen). Standard-
  Mittelspiel zeigt deutlich anderes Bild (Rest-Gap −564 cp) —
  separates Thema, nicht hier. Konsequenz: A/B-Match
  `connected_rooks_pair = 30` vs 150 gestartet. Logs:
  `validate_connected_rooks_2026-05-15.txt`,
  `eval_breakdown_w5abogf0_2026-05-15.txt`,
  `quiesce_trace_w5abogf0_2026-05-15.txt`.
- **Cluster-1-Stichprobe `missed_capture` (15.05.2026) — DONE.**
  9 Stellungen aus dem 15.05-Auswertungsfenster auf Engine-Verhalten
  abgeklopft (`tools/probe_missed_captures.py`, je 30 s / d ≤ 14):
  3/9 lösen sich ab d4–d10 (Such-/Zeitdruck-Problem, kein Eval-Bug),
  4/9 bleiben bis d10 falsch, 2/9 oszillieren. Sanity-Check via
  Stockfish d30 (`tools/verify_cluster1b_stockfish.py`) reduziert die
  4 echten Kandidaten auf **2 belastbare Bugs** — `m1oQlfmG` war
  Artefakt der d17-Momentaufnahme (Stockfish flippt bei d30 selbst auf
  `Be2`). Übrig: `54iwUiMx` m25 (`Nxd4`, SF-Score läuft −66→0 cp) und
  `W5AboGf0` m29 (`Qxd6`, −62→−32 cp). Deep-Dive an W5AboGf0
  (`tools/trace_w5abogf0.py`): Free-Search 120 s / 100 M Knoten
  erreicht d10 und bleibt bei Qg4 (−49 cp); Post-Move-Eval pro Kandidat
  einzeln zeigt Qxd6 = +47 cp vs Qg4 = −44 cp (Diff 91 cp). SEE
  rechnet Q-für-Q-Capture korrekt mit Netto 0. **Befund: Search-
  Tiefe-/Move-Ordering, kein statischer Eval-Bug.** Daraus die
  Priorisierung in "Nächste Schritte": Aspiration Windows hochgezogen.
  Logs: `probe_cluster1_2026-05-15.txt`,
  `verify_cluster1b_sf_2026-05-15.txt`,
  `trace_w5abogf0_2026-05-15.txt`.
- **Buchlücken-Patches stickshark99-Block (15.05.2026) — DONE.**
  Vier zusätzliche Einträge in `tools/build_book_patches.py` →
  `src/polyglot/martuni_patches.bin` (jetzt 6 Einträge): `7…c5` statt
  `Bxc3+` (J6OoNBPQ), `10…Nge5` statt `Nxf2` (B8ZDeiEi), `11…a5` statt
  `O-O-O` (PLLnwdrh), `12.Nb5` statt `Nc4` (FdSEbZbU). Anders als die
  bisherigen Patches einmalige FENs aus 16 stickshark99-Partien — Wert
  niedriger, aber für frühe Mittelspielzüge plausibel; Reviewhinweis
  im Patch-Kommentar (entfernen falls keine Recurrence).
- **Auswertung 15.05.2026 (144 Partien post Dynmat-Step2 v2) — DONE.**
  Sample 13.05. (post-Deploy) — 15.05. 14:40. Lichess Blitz 2027 → 2010
  (−17), Rapid 2075 → 2083 (+8); netto flach im Rauschen, kein Rollback-
  Trigger erfüllt. **Entscheidung: Step 2 v2 behalten.** Σ 1.69
  Blunder/Partie (243 total). `missed_mate` stabil 0.035/P,
  `allows_mate` 0.188/P ist Artefakt aus bereits verlorenen Stellungen
  (alle 27 Fälle `eval_before < −300cp`, 7 davon aus einem einzigen
  Endspiel-Shuffling-Spiel `pD9ZAV3G`). `hangs_bishop` 0.109 → 0.139/P
  leichter Drift (11 in offenem Spiel), im Auge behalten. Neue
  Auffälligkeit: `missed_capture` 0.181/P, 13 davon in offenem Spiel —
  zwei Cluster (Capture-vs-Capture-Ordering und Forcing-Sac-Pruning).
  Neuer Sparring-Hotspot `stickshark99` (16 Spiele, 2.25 B/P, davon 5
  Eröffnungspatzer); `sxphia` von 1.7 auf 1.42 B/P gefallen. Datei:
  `analyse-13.05.2026.json`. Backup-Binary
  `target/release/martuni.backup-step1-20260513` bleibt aufgehoben.
- **Auswertung 11.05.2026 (137 Partien post Dynmat-Step1) — DONE.**
  Sample 10.05 22:17 — 12.05 19:05, alle mit Dynmat-Step1 live. Mate-
  Metriken weiter gesunken: `missed_mate`/Partie 0.038 → **0.029**,
  `allows_mate` 0.109 → **0.080**. Roadmap-Vorhersage eingelöst:
  `hangs_bishop` 0.120 → **0.109** (−9 %), `trade_down` 0.082 → 0.088
  ≈ flat. Lichess Blitz 1969 → 2027 (+58), Rapid 2032 → 2075 (+43). Σ
  Blunder/Partie 1.23 → 1.98 (+61 %), aber Gegner-Mix verzerrt
  (HynobiusChess 19×, black_numba 15×, TiSchachBot 13× → Bot-Turnier mit
  wenigen Teilnehmern, höheres Rating zieht härtere Stellungen). `hangs_
  knight` 0.066 → 0.190 ist überwiegend MG-Taktik in bereits gewonnenen
  Stellungen, kein Trade-Calculus-Effekt — vor Step 2 keine Eval-
  Reaktion. Datei: `analyse_11.05.2026.txt`.
- **Buchlücken-Patch EpimetheusBot 11. Nbd4 (12.05.2026) — DONE.**
  Zweiter Eintrag in `tools/build_book_patches.py` /
  `src/polyglot/martuni_patches.bin` (Polyglot-Hash
  `0xecaa75f8ae670fcc`): in `r1bk1b1r/p3pppp/2p2n2/1N6/1nP5/5N2/PP3PPP/
  R1B1KB1R w KQ - 0 11` spielt Martuni jetzt **Nbd4** statt **Na3**. Das
  alte `Na3` lief 5× in 137 Partien (≈200 cp Verlust pro Partie) — neben
  dem sxphia-Bxc5-Eintrag der zweite EpimetheusBot-typische Wiederholer.
  Verifikation: `info string book hit / bestmove b5d4`.
- **Dynamische Figurenbewertung Schritt 1 (10.05.2026) — DONE.** MG/EG-
  Tapering nur für Springer und Läufer im Per-Figur-Materialscore
  (`piece_material(piece, p, phase)` in `eval.rs`). Statische `p.knight` /
  `p.bishop` bleiben Anker für `king_exposure_penalty` (NPM-Schwelle 1500cp)
  und `endgame::strong_material` — Eigene Vorgabe von Tobias, damit der A/B-
  Test sauber ist und nicht versehentlich indirekte Eval-Drift entsteht.
  Neue Sektion `[material_dynamic]` in `eval.toml` mit Werten
  knight_mg=310 / knight_eg=290 / bishop_mg=305 / bishop_eg=320 — Kaufman-
  konsistent: N–B-Differenz ≈ 0 cp im MG, ≈ −30 cp im EG. Defaults im Code
  sind 300/300/300/300, ohne Override neutral. Mess-Setup: fastchess
  (`~/tools/fastchess`) + UHO_Lichess_4852_v1.epd (`~/tools/openings`).
  A/B-Match 1000 Partien (5+0.05, UHO, fastchess SPRT [0,10]):
  Dynmat +11.47 Elo ± 17.30, LOS 90.35 %, 402W / 163D / 435L Baseline-
  Sicht. SPRT formal nicht entschieden (Effekt unter +10-Schwelle), aber
  positive Tendenz konsistent mit dem 200-Spiele-Vorlauf (+15.65 Elo) — als
  bestanden gewertet. Schritte 2 (Pawn-Adjustment) und 3 (dynamisches
  Bishop-Pair) zurückgestellt bis Lichess-Validierung. Match-Outputs
  unter `matches/baseline_vs_dynmat_step1*/` (gitignored).
- **Buchlücken-Patch (10.05.2026) — DONE.** `src/polyglot/martuni_patches.bin`
  als Polyglot-Datei mit Vorrang vor den externen Büchern. Erster Eintrag:
  `9...Bc7 statt 9...Bxc5` in der wiederkehrenden sxphia-Stellung
  (`r1bq1rk1/1p1n1ppp/p1pbpn2/2Pp4/3P2P1/2N1PN1P/PPQ2P2/R1B1KB1R b KQ - 0 9`),
  Martuni hatte dort viermal in Folge denselben hangs_bishop-Blunder
  gespielt (Verlust ~270 cp pro Partie). Generator: `tools/build_book_patches.py`
  (python-chess, gegengeprüft am Polyglot-Startpos-Hash). `.gitignore`-
  Ausnahme für die Patch-Bin, externe Bücher bleiben ungetrackt.
- **Hotpath-Cleanup (09.05.2026) — DONE.** PR `perf-hotpath-logic-cleanup`,
  ausgelöst durch Beobachtung in Shredder-GUI auf dem Windows-Host: Martuni
  blieb bei Tiefe 7, Komodo 12 schon bei 17. Codex-PR im Review identifiziert
  und gemerged. Wesentliche Änderungen:
  - **Redundante MoveGen** in `alpha_beta` und `quiescence` entfernt:
    `board.status()` baut intern selbst `MoveGen::new_legal()`, danach lief
    derselbe Generator nochmal fürs Ordering. Jetzt: einmalige
    Generation, Terminal-Erkennung anhand `legal_moves.len() == 0` (vor NMP
    eingehängt, damit Stalemate/Checkmate nicht durchs Null-Move-Pruning
    fallen).
  - **Repetition-/TT-Hash** von `polyglot_hash` (iteriert alle 64 Felder) auf
    `Board::get_hash()` umgestellt — die `chess`-Crate pflegt diesen
    Zobrist-Hash inkrementell in `make_move`. Polyglot-Hash bleibt nur
    fürs Eröffnungsbuch zuständig.
  - **`check_ext`** wird nicht mehr unkonditional berechnet (sondern nur,
    wenn das Kind tatsächlich im Schach steht).
  - **`is_passed`/`is_passed_simple`** und **`rooks_connected`** auf
    Bitmask-Operationen umgebaut, kein `Vec`/`Square`-Loop mehr.
  - **`endgame::signature`** mit Fast-Exit `pawn_total > 1 → None`.
  - **`target-cpu=native`** korrekt nach `.cargo/config.toml` als rustflag —
    in `[profile.release]` wurde der Schlüssel von Cargo ignoriert
    (Compiler-Warnung "unused manifest key").
  - Verifikation Mittelspiel-Stellung `r4rk1/1bqnppbp/p2p1np1/1pp5/3PP3/
    2NBBN1P/PPPQ1PP1/R4RK1 w` mit `go movetime 12000`: Master Tiefe 6 in
    8.7 s (Tiefe 7 nicht erreicht), neuer Build Tiefe 7 in 9.1 s. NPS
    ≈1.6 M → ≈2.8 M (+64 %). Scores und Best-Moves auf jeder Tiefe
    identisch — reine Geschwindigkeit, keine Logikänderung.
- **Auswertung 09.05.2026 (183 Partien) — DONE.** Tiefenrelevante Motive
  weiter rückläufig: `allows_mate`/Partie 0.126 (04.05) → **0.109** (-13 %);
  `missed_mate`/Partie 0.057 → **0.038** (-33 %, sehr nah am ursprünglichen
  Ziel 0.03). Lichess Blitz 1965 → 1969 (+4), Rapid 2016 → 2032 (+16) —
  unterhalb der LMR-Zielspanne +30 bis +60, aber das Mess-Intervall enthält
  zwei zwischenzeitliche Bug-Fixes (Repetition-Detection 02.05, TT-Cutoff
  07.05) plus den Compiler-Tweak vom 06.05; klare Attribution
  schwierig. Neue Hotspots im Mittelspiel: `unclassified` 91 (40 %),
  `positional_collapse` 27, `hangs_bishop` 22 — typischer Mittelspiel-
  Bodensatz, kein klares Strukturproblem. Datei: `analyse_09.05.2026.txt`.
- **TT-Cutoff bei Repetition-Vergiftung (07.05.2026) — DONE.** Reproduzierer
  PGQZhMjF (Wojtmic-Bot vs Martuni, 06.05.): die TT speichert nur den
  Stellungs-Schlüssel, keinen Repetition-Kontext. Wenn dieselbe Stellung im
  Spielverlauf bereits aufgetaucht war, kann ein gespeicherter Mate-/
  Cutoff-Score stale werden — der frühere Pfad führte zu echtem Mate, der
  jetzige läuft durch eine 3-fold-Wiederholung. Fix in
  `alpha_beta`: bei TT-Hit prüfen, ob `key` im Slice
  `state.history[..root_history_len]` (Spielhistorie) liegt; wenn ja, nur
  Move-Hint übernehmen, Score-Cutoff unterdrücken. `slice.contains` ist
  O(n) auf einem Slice typisch < 200 Einträge und wird nur betreten, wenn
  überhaupt ein Cutoff-Kandidat anliegt — kein Hot-Path-Treffer. Spiegelbild
  des Repetition-Bugs vom 02.05.: die Repetition-Logik ist in beide
  Richtungen heikel. Tests: `search::tests::poisoned_tt_does_not_select_
  repeated_move`, `tt_cutoff_suppressed_when_key_in_game_history`.

- **Auswertung 01.05.2026 (162 Partien) — DONE.** Die 28.04-Anpassung hat
  geliefert: Endgame-Blunder/Partie 0.60 → 0.358, exposed_king 0.14 → 0.086,
  positional_collapse 0.24 → 0.160 (alle deutlich besser). Einziger
  Negativtrend: `missed_mate`/Partie 0.075 → 0.105. Inspektion der 17 Fälle
  zeigt: nahezu alle sind Stellungen, in denen Martuni schon klar gewann
  (`martuni=+6cp .. +21cp`) und nur das schnellste Matt nicht fand —
  strukturelles Tiefen-Problem, kein Eval-Fehler. Lichess-Rating: Blitz
  1864 → 1921, Rapid 1928 → 1975 (3 Tage). Befund hat NMP-Implementierung
  ausgelöst.
- **Eingemauerter Turm (`rook_trapped_endgame_penalty`) — DONE.** Am 28.04.
  mit `tools/diagnose_rook_trapped.py` geprüft: Term feuert in nur 2 von 104
  Endgame-Blundern (1.9 %), in beiden Fällen sachlich korrekt. Nicht der
  Treiber der Endgame-Verschlechterung — bleibt bei `-10`, kein Anlass zum
  Justieren.
- **Null-Move Pruning + PVS — DONE 01.05.2026.** Plan stand in
  [null-move-pruning.md](null-move-pruning.md) (NMP-Konzept), PVS wurde
  gleich mitgeliefert, weil NMP ohne Nullfenster-Knoten in der Suche nie
  greift. Verifikation: Mittelspiel-Stellung −43 % Knoten auf gleicher
  Tiefe; `missed_mate`-Stellung aus dem Analyse-File (Martuni vs Bot5551,
  Zug 29) wird jetzt mit `mate 6` gelöst statt vorher unentdeckt zu
  bleiben. R = 2 konstant, Mindesttiefe 3, Zugzwang-Schutz via
  `has_non_pawn_material`.
- **Auswertung 04.05.2026 (175 Partien) — DONE.** NMP-Effekt bestätigt:
  `missed_mate`/Partie 0.105 → **0.057** (−46 %), Lichess Blitz 1921 → 1965
  (+44), Rapid 1975 → 2016 (+41). Endgame-Rate stabil (0.358 → 0.337),
  keine Zugzwang-Regression. Neuer Hotspot `allows_mate` 0.126/P. (22 Fälle),
  primär Tiefenproblem in bereits verlorenen Stellungen. Details:
  `project_auswertung_2026_05_04` Memory, Datei `analyse_04.05.2026.txt`.
- **LMR implementiert (04.05.2026).** Variante A nach Tobias-Spezifikation:
  Stufenformel (R=1 ab depth≥3 & Index≥3, R=2 ab depth≥6 & Index≥6),
  nur Non-PV-Knoten, ab dem 4. sortierten Zug. Ausgeschlossen von Reduktion:
  Captures (über `sm.see_val`), Promotionen, Schachgebote, Züge im Schach,
  Killer-Moves, alle Züge mit aktiver Extension. Re-Search-Kaskade:
  reduzierte Nullfenster-Suche → bei Fail-High volle Tiefe Nullfenster →
  bei `alpha < score < beta` PVS-Re-Search mit vollem Fenster.
  Verifikation: `missed_mate`-Stellung Tiefe 9/mate 6 in 2.7 s mit 6.8 M
  Knoten (vorher Tiefe 7/mate 6 in 5.7 s mit 17.4 M Knoten). Wichtig beim
  Implementieren war: `.max(1)` auf `scout_depth` darf nur greifen, wenn
  tatsächlich reduziert wird, sonst wird der natürliche Übergang
  `new_depth==0` → Quiescence aufgebläht und die Suche kollabiert.
  Konzeption: [lmr-plan.md](lmr-plan.md). History-Heuristic bewusst NICHT
  als zusätzliches LMR-Kriterium — wirkt nur über die Zugreihenfolge.
- **Repetition-Detection korrigiert (02.05.2026).** `state.history.contains`
  zählte vorher 1-fold in Spielhistorie als Remis und blockierte ruhige
  Best-Moves (Repro: vGwmaXUy, 19.Ng5?? statt 19.Qe4). Neuer Helfer
  `is_repetition_draw` trennt Spielhistorie und Suchpfad
  (`SearchState.root_history_len`); Unit-Tests in `search::tests`.
- **Analyse-Skript verbessern — DONE 02.05.2026.** Pauschales
  `--min-movetime 0.3` ersetzt durch per-Klasse-Schwellen über
  `est = base + 40·inc` (Lichess-Konvention): bullet 0s, blitz 1s, rapid 3s,
  classical 5s. Schwellen sind `int` (PGN-Clocks haben Sekunden-Auflösung),
  `--min-movetime` (float, Default 0.3) bleibt als Fallback für fehlende /
  unparsbare `[TimeControl]`-Tags. Skip-Meldung auf stderr zeigt jetzt Klasse
  + Schwelle. Doku: [blunder-analyse.md](blunder-analyse.md) (Wartungshinweis
  „Movetime-Filter pro Zeitkontrolle (2026-05-02)").
- **Mobility-Term — DONE.** Variante B (Safe Mobility) in
  `eval.rs::mobility_score` implementiert; getapert zwischen MG/EG mit
  Defaults `knight 3/3, bishop 3/4, rook 2/5, queen 1/2`. Eingeführt nach
  der Analyse vom 21.04.2026 als Antwort auf den Mittelspiel-Bodensatz aus
  `unclassified` und `positional_collapse` (siehe [see.md](see.md),
  Abschnitt „Regression-Analyse 2026-04-21").
- **Turm auf 7. Reihe — DONE.** `eval.rs::rook_seventh_rank_bonus`.
- **SEE + Bad-Capture-Pruning + Killer/History — DONE.** Lange Mess- und
  Korrekturhistorie in [see.md](see.md) (April-Iterationen 12. → 21.).
- **Endspielmodul Phasen A/B/C — DONE.** Mop-up (KRvK, KQvK, KRRvK, KQQvK),
  KPK mit Square-Rule, KBNK mit Bishop-Color-Mattecken; siehe
  [endgame.md](endgame.md) und [endgame.rs](../src/endgame.rs).
