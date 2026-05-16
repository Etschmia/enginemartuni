# MVV-Bonus bei SEE = 0 — Konzept für Martuni

**Status:** **VERWORFEN nach A/B-Match 16.05.2026.** Variante A
(Centipawn-MVV mit LVA-Modifier) implementiert, Smoke + fastchess-SPRT
1000 Partien gefahren — Ergebnis +6.60 ± 16.76 Elo (LOS 78 %, SPRT
nicht entschieden) plus Smoke-Regression auf der Cluster-1b-Anker-
Stellung W5AboGf0. Zu schwach für Rollout. Code wieder aus
`src/search.rs` entfernt (Build reproduzierbar — neuer `martuni`-Hash
identisch zur pre-MVV-Baseline). Befund-Block ganz unten.

**Erwarteter Elo-Gewinn (urspr. Annahme):** +0 bis +10 Elo. Lokaler
Move-Ordering-Hebel, nicht der ganz große Tiefen-Schub. Realistisch
war das Ziel, ein bis zwei der noch offenen `missed_capture`-Cases
aus dem Smoke-Test aufzulösen, ohne die anderen sechs zu
verschlechtern. — Smoke löste zwei, brach eine; A/B konnte den Netto-
Effekt nicht klar machen.
**Betroffenes Modul:** ausschließlich `src/search.rs`, konkret
`order_moves` (Zeile ~1100) und `mvv_lva_key` (Zeile ~1174).
**Voraussetzung:** keine. Eigenständige Move-Ordering-Verfeinerung.

## Was hat sich seit der Roadmap-Notiz geändert

Die Roadmap-Notiz vom 14.–15.05. nennt **W5AboGf0** als Anker-Beispiel
("Qxd6 sortiert sich aktuell wie ein 'neutraler Schlag'"). Smoke-Test
heute (16.05., nach CR30-Rollout) zeigt aber:

```
W5AboGf0 m29 W  | OFF (Aspiration off, normale Suche)  | maxD=9 | best=e6d6 (Qxd6) | (G)
```

Die Engine findet `Qxd6` jetzt schon ohne weitere Änderung. Das CR30-
Rollout ([[project-ab-conn-rooks]]) hat den Eval-Bias entschärft, der
die Bewertung der Folgeposition nach Qxd6 verzerrt hatte — siehe
`eval_breakdown_w5abogf0_2026-05-15.txt`: dort war `connected_rooks
Diff=+150` ein dominanter Faktor, nach CR30 nur noch +30.

**Damit verschiebt sich der Hebel-Zielpunkt:**
- W5AboGf0 ist *gelöst*, MVV-Bonus muss dort nichts mehr leisten.
- Offen aus dem Smoke (OFF-Spalte) bleiben: `m1oQlfmG` (B), `54iwUiMx`
  (?), `wo4G1Ae5 m15` (?), `wo4G1Ae5 m19` (?). Diese Stellungen sind
  die neue Mess-Basis.

## Wie sortiert order_moves heute Captures

Aktuelle Hierarchie in `search.rs:1091–1099` (niedriger Key = zuerst):

```
TT-Move:                 -100_000
Promotion zu Dame:        -50_000
Gewinnender Capture:      -40_000 + mvv_lva_key       (SEE >= 0)
Killer 1:                 -30_000
Killer 2:                 -25_000
Unterpromotion:           -20_000
Quiet Move (History):     -history    Range [-16_000, 0]
Verlierender Capture:     +10_000 - SEE               (SEE < 0)
```

Mit `mvv_lva_key = -(victim_rank * 10 - attacker_rank)` und
`piece_rank` ∈ {P=1, N=B=3, R=5, Q=9, K=100}.

**Ranges im gewinnenden Capture-Tier:**

| Capture-Typ | victim_rank | attacker_rank | mvv_lva_key | order_key |
|---|---:|---:|---:|---:|
| Pxe7 (P für Q) | 9 | 1 | −89 | **−40_089** |
| Qxd6 (Q für Q) | 9 | 9 | −81 | **−40_081** |
| Rxd5 (R für R) | 5 | 5 | −45 | −40_045 |
| Nxe4 (N für N) | 3 | 3 | −27 | −40_027 |
| Bxc4 (B für B) | 3 | 3 | −27 | −40_027 |
| Nxe4 (N für P) | 1 | 3 | −7 | −40_007 |
| Pxd5 (P für P) | 1 | 1 | −9 | −40_009 |

Beobachtung: Der Spread innerhalb des Tiers ist nur ~82 Punkte breit
(−40_089 bis −40_007). Damen-/Turm-Tausche kommen zwar **rechnerisch**
vor Bauern-Tauschen, aber sie liegen alle in einem schmalen Band
**hinter** TT-Move (−100_000) und Promotion (−50_000) — und vor dem
Killer-Tier (−30_000). Innerhalb dieser ~10_000 Punkte zwischen
"Promotion" und "Killer 1" liegt die gesamte Capture-Logik.

## Was Tobias' Idee aus der Roadmap leistet

Die Intuition: SEE = 0 Captures mit **wertvollem** Opfer (Q oder R)
sollen klar getrennt von SEE = 0 Captures mit **billigem** Opfer (P)
sortiert werden — und zwar mit deutlich mehr Spreiz als die heutigen
~80 Punkte. Hintergrund: ein Q-Tausch räumt 1800 cp Material vom Brett
und vereinfacht die Folge-Stellung dramatisch. Ein P-Tausch räumt
200 cp und ändert kaum was. Das MVV-Ranking heute behandelt sie als
"gleicher Tier, kleine Sortierungs-Differenz". Tobias' These: das ist
zu schwach gewichtet.

Zwei Wirkweisen sind plausibel:
1. **Wurzel-Reihenfolge ohne TT-Hint** (erste ID-Iteration oder
   neue Position): high-MVV-Captures kommen klarer als allererstes —
   damit volles Fenster, damit korrekter Score.
2. **PVS-Scout-Stabilität** an inneren Knoten: wenn high-MVV-Captures
   früher gescoutet werden, wird ihr Score öfter zu `alpha` und der
   restliche Baum profitiert von engerem Pruning.

## Drei Varianten

Reihenfolge: von konservativ nach aggressiv. Tobias entscheidet, ich
mache anschließend nur eine davon.

### Variante A — Centipawn-MVV mit LVA-Modifier (Empfehlung)

`mvv_lva_key` ersetzen durch eine Funktion in **Centipawn-Werten**
statt Rang-Werten:

```rust
// Statt: -(victim_rank * 10 - attacker_rank)  mit ranks 1/3/3/5/9/100
// Neu:    -(victim_cp * 10 + (1000 - attacker_cp))
//
// victim_cp:    P=100, N=320, B=330, R=500, Q=900, K=20000
// attacker_cp:  P=100, N=320, B=330, R=500, Q=900, K=20000
fn mvv_lva_key_cp(board: &Board, mv: ChessMove) -> i32 {
    let victim   = board.piece_on(mv.get_dest()).map(piece_cp).unwrap_or(100);
    let attacker = board.piece_on(mv.get_source()).map(piece_cp).unwrap_or(0);
    -(victim * 10 + (1000 - attacker))
}
```

Damit:

| Capture | victim_cp | attacker_cp | neuer key | alter key (Δ) |
|---|---:|---:|---:|---:|
| Pxe7 (P für Q) | 900 | 100 | **−9_900** | −89 (×111) |
| Qxd6 (Q für Q) | 900 | 900 | **−9_100** | −81 (×112) |
| Rxd5 (R für R) | 500 | 500 | **−5_500** | −45 |
| Nxe4 (N für N) | 320 | 320 | **−3_880** | −27 |
| Nxe4 (N für P) | 100 | 320 | **−1_680** | −7 |
| Pxd5 (P für P) | 100 | 100 | **−1_900** | −9 |

Spread im Tier vergrößert sich von ~80 auf ~8_000 Punkte. Q-/R-
Captures rücken weit vor P-/N-Captures, **innerhalb** des Capture-
Tiers — aber Promotion zu Q (−50_000) und TT-Move (−100_000) bleiben
weiter klar vor allen Captures. Die Skala fügt sich also ein, ohne
andere Tiers umzuwerfen.

Risiko: bei den seltenen Spezialfällen (Promotion-Capture, en-passant)
muss die Funktion eine konsistente Bewertung liefern — siehe Detail-
Punkt unten.

### Variante B — additiver Bonus nur bei SEE = 0

Den heutigen `mvv_lva_key` lassen und einen separaten Bonus addieren,
wenn der Capture exakt SEE = 0 hat **und** das Opfer mindestens ein
Springer/Läufer ist:

```rust
if v >= 0 {
    let mut key = -40_000 + mvv_lva_key(board, mv);
    if v == 0 {
        let victim = board.piece_on(mv.get_dest()).map(piece_cp).unwrap_or(0);
        if victim >= 300 {
            key -= victim * 5;   // Q-Tausch: −4500, R-Tausch: −2500, N/B-Tausch: −1500
        }
    }
    key
} else {
    10_000 - v
}
```

Kleiner Eingriff, sehr lokal, gut testbar. Wirkt nur auf die
spezifische Klasse, die Tobias in der Roadmap nennt. Risiko: könnte
zu schwach sein — ein Bonus, der nur SEE = 0 trifft, ändert nichts an
SEE > 0 Captures, und je nach Stellung sortiert ein P-für-N (SEE=+220)
weiterhin vor einem Q-für-Q (SEE=0, mit Bonus).

### Variante C — SEE-Wert direkt in den Sortier-Key

Den `mvv_lva_key` durch den SEE-Wert ersetzen und MVV nur als
Tie-Breaker behalten:

```rust
if v >= 0 {
    -40_000 - v * 10 + (mvv_lva_key(board, mv) / 10)
} else { ... }
```

Maximal radikal — sortiert "gewinnenden" Capture nach **Netto-Material-
Gewinn** statt nach Tausch-Wert. P-für-Q (SEE=+800) käme weit vor
Q-für-Q (SEE=0). Das ist *strategisch* nicht unbedingt richtig: bei
gleichem SEE = 0 würde Q-für-Q und B-für-B gleich sortiert, was den
"Damen-Tausch räumt das Brett"-Gedanken wegwirft. Tobias' Roadmap-
Intuition richtet sich **gegen** diese Variante; ich liste sie nur,
weil sie der vollständigkeitshalber im Optionsraum liegt.

## Details, die in allen Varianten dieselbe Aufmerksamkeit brauchen

1. **En passant**: `board.piece_on(mv.get_dest())` liefert `None`,
   weil das Zielfeld leer ist. Heute fallen wir auf
   `piece_rank.unwrap_or(1)` zurück (Bauer geschlagen, Rang 1). Im
   neuen Code dieselbe Fallback-Logik einhalten.
2. **Promotion-Capture** (Bauer schlägt mit Promotion): heute wird das
   als regulärer Capture behandelt, mit Promotion-Bonus durch das
   "Promotion zu Dame"-Tier *nicht* erfasst (weil das Tier nur für
   Quiet-Promotion greift — Promotion-Captures landen im Capture-
   Tier mit `attacker_rank=1`). Variante A würde das automatisch
   korrekt einsortieren (P für Q mit Promotion: victim_cp=900,
   attacker_cp=100). Variante B würde greifen, falls SEE = 0 (selten
   bei Promotion-Captures, weil der gepromotete Bauer ja Q wert ist).
3. **TT-Move-Pfad**: wenn der TT-Move selbst ein Capture ist (Zeile
   1110–1120), wird er mit Key −100_000 sortiert **unabhängig vom
   MVV-Wert**. Das bleibt unverändert in allen Varianten — TT-Move
   gewinnt immer.

## Test-Plan

Wie bei Aspiration: erst Smoke, dann A/B, dann Lichess-Lookback.

1. **Smoke-Test.** `tools/probe_aspiration.py` ist schon parametrisch
   auf die 9 Cluster-1b-Stellungen — ich passe das Skript für MVV-
   Vergleich an (env-Toggle `MARTUNI_MVV_VARIANT=baseline|cp|see0bonus`
   o.ä., oder zwei Binaries) und schau auf:
   - findet die Engine `m1oQlfmG` jetzt cxd5 statt Nxe4?
   - bricht `H62xk9vz`, `zcBr22eo`, `25eZUsMT`, `FLSJc0Sm` (alle heute
     G in OFF) — und vor allem `W5AboGf0` (heute G nach CR30) — durch
     die Änderung?
   - max-depth pro Stellung: ON ≥ OFF (Erwartung neutral oder leicht
     besser, da Move-Ordering keine Tiefe kostet)
2. **A/B-Match.** Standard fastchess-Setup
   ([[reference-match-runner]]): UHO_Lichess_4852_v1, 5+0.05,
   SPRT [0, 10], 1000 Partien. Backup-Binary anlegen vor Rollout.
3. **Lichess-Lookback.** 100–150 Partien nach Deploy.
   Blunder-Metrik: `missed_capture` sinkt von heute 0.203/P
   ([[project-auswertung-2026-05-16]]) auf ≤0.17. Bei Plateau:
   verworfen, Rollback aufs Backup-Binary.

## Drei offene Entscheidungen

1. **Variante A / B / C?** Empfehlung A (Centipawn-MVV mit LVA-
   Modifier). Begründung: B ist zu schmal angesetzt (greift nur bei
   SEE = 0, ignoriert die heute schon zu schmale Capture-Tier-
   Spreizung generell); C kippt MVV-Logik um eine Achse, die
   Tobias in der Roadmap explizit *nicht* wollte.
2. **Smoke-Test-Toggle**: env-Variable wie bei Aspiration
   (`MARTUNI_MVV=cp|baseline`) — oder ein zweites Binary
   (`martuni-mvv-cp`)? Env ist kleiner Eingriff im Code, zweites
   Binary ist sauberer für den eigentlichen A/B-Match.
3. **Probe-Skript**: `tools/probe_aspiration.py` umparametrisieren
   (gemeinsames `tools/probe_capture_ordering.py`?) oder
   neues `tools/probe_mvv.py` daneben? Erstere Variante ist DRY,
   letztere lässt die Aspiration-Reproduzierbarkeit unangetastet.

Tobias' Wahl (16.05.2026): Variante A, zweites Binary, Skript
generalisiert zu `tools/probe_capture_ordering.py`.

---

## Smoke-Test-Befund (16.05.2026, vor A/B)

Ausgeführt: `tools/probe_capture_ordering.py` über 9 Cluster-1b-
Stichproben, movetime 30 s, max_depth 16, je `martuni.backup-pre-mvv-
20260516` (Baseline) gegen `martuni-mvv-cp` (Challenger).
Rohlog: `logs/probe_capture_ordering.out`.

```
Quality:  Base  G=4 B=2 ?=3       Chal  G=5 B=2 ?=2     (+1 G, −1 ?)
Σ maxD :  Base 82 (Ø 9.11)         Chal 84 (Ø 9.33)      (+2 plies)
```

Verbesserungen (Base nicht-G → Chal G):
- `H62xk9vz m14`: Kxd2 (B) → Qxd2 (G), maxD 8 → 9
- `wo4G1Ae5 m15`: f3d2 (?) → exf5 (G), maxD 8 → 9

**Regression** (Base G → Chal nicht-G):
- `W5AboGf0 m29`: Qxd6 (G, d9) → Qg4 (B, d10) — die historische
  Cluster-1b-Anker-Stellung, die nach CR30-Rollout heute morgen
  zurückgewonnen worden war.

## A/B-Match-Befund (16.05.2026, 1000 Partien)

Setup: `matches/baseline_vs_mvv_cp/run.sh`, fastchess 1.8.1 SPRT
[0, 10] α=β=0.05, 5+0.05, UHO_Lichess_4852_v1, concurrency 2,
Hash 64 MB. Rohlog: `matches/baseline_vs_mvv_cp/run.log`.

```
Results of Baseline vs MVVcp:
  Elo: -6.60 +/- 16.76,  nElo: -8.49 +/- 21.53
  LOS: 21.98 %  (= 78.02 % für MVVcp)
  DrawRatio: 49.20 %,  PairsRatio: 0.90
  Games: 1000   W: 411   L: 430   D: 159   Points: 490.5 (49.05 %)
  Ptnml(0-2): [61, 73, 246, 64, 56],  WL/DD Ratio: 21.36
  LLR: -1.12 (-37.9%) (-2.94, 2.94) [0.00, 10.00]
  Total Time: 01:45:10
```

Aus Sicht MVVcp: **+6.60 ± 16.76 Elo, LOS 78 %, SPRT nicht
entschieden** (LLR −1.12, leicht Richtung H0).

Vergleich mit historischen A/Bs:

| Patch | A/B Elo ±CI | LOS | Entscheidung | Lichess |
|---|---|---:|---|---|
| Dynmat Step 1 | +11.47 ±17.30 | 90 % | ausgerollt | +77 / +20 |
| Dynmat Step 2 v2 | +5.56 ±18.89 | 72 % | ausgerollt | flat |
| **MVV-CP** | **+6.60 ±16.76** | **78 %** | **VERWORFEN** | — |

## Konsequenz

- **MVV-CP-Code aus `src/search.rs` entfernt.** Rebuild produziert
  Binary mit MD5 identisch zur pre-MVV-Baseline → reproduzierbarer
  Revert.
- `target/release/martuni-mvv-cp` als Reproduktions-Binary
  aufgehoben (für ev. spätere Re-Analyse / Variante B-Vergleich).
- **Begründung Verwerfen** (gegen die Step-2-v2-Rollout-Disziplin):
    1. [[feedback-ab-vs-lichess-signal]] gilt primär für Eval-Hebel,
       wo Selfplay-Spiegelstil das Stilspektrum unterzählt. MVV ist
       Move-Ordering — Selfplay sollte hier ehrlicher sein. +6.60 Elo
       Nominell mit CI über Null ist dann **schwächer**, nicht
       gleichwertig, gegenüber dem Step-2-Profil.
    2. Smoke-Regression auf `W5AboGf0` ist ein konkretes
       Negativ-Signal an einer historisch bekannten Stellung. Step 1
       und Step 2 v2 hatten kein vergleichbares Warnsignal vor dem
       Rollout — bei beiden war es ausschließlich "A/B knapp positiv".
- **Roadmap-Konsequenz:** MVV-Punkt aus Position 1 raus. Roadmap-
  Punkt 3 (CR30) ist bereits ausgerollt, also bleibt als nächster
  echter Hebel **Dynmat Step 3 — dynamisches Bishop-Pair**, oder
  AetherBot-Lookback ([[project-auswertung-2026-05-16]]).

## Was bleibt im Repo

- `docs/mvv-bonus.md` mit Status VERWORFEN (dieses Dokument)
- `tools/probe_capture_ordering.py` — generisches Move-Ordering-
  Smoke-Skript für künftige Experimente (zwei Binary-Pfade als
  Argumente, 9 Cluster-1b-Stellungen, Quality+maxD-Vergleich)
- `matches/baseline_vs_mvv_cp/` — Match-Setup + Rohdaten für ev.
  spätere Vergleichs-Auswertung
- `target/release/martuni-mvv-cp` — Challenger-Binary, aufgehoben

## Variante B als möglicher Folge-Versuch

Nicht aktiv eingeplant, aber falls die Klasse "Q-Tausch räumt
Brett" später nochmal aufkommt: Variante B (additiver Bonus nur bei
SEE = 0 mit victim ≥ N, +victim_cp × 5) ist der kleinere Eingriff
und könnte die W5AboGf0-Regression vermeiden. Würde wieder einen
Smoke + A/B brauchen (~2 h Engagement).
