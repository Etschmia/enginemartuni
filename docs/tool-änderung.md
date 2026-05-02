# Tool-Änderung — `tools/analyze_blunders.py` mit dynamischen Movetime-Schwellen

**Status:** umgesetzt am 02.05.2026 (vor der für „nach >100 Partien" geplanten
Auswertung). Laufende Doku in `docs/blunder-analyse.md` (Wartungshinweis
„Movetime-Filter pro Zeitkontrolle (2026-05-02)").
**Anlass:** 28.04.2026 — Klärung der Auflösungs-/Filter-Semantik.

## Kontext

Lichess-PGNs aus `~/lichess-bot/game_records/` haben `[%clk H:MM:SS]` —
**Sekunden-Auflösung, keine Zehntel.** Damit ist die Differenz zwischen zwei
Uhren­ständen ein­und­derselben Seite immer ganzzahlig (`0.0`, `1.0`, …).

Der bisherige Filter `time_spent < min_move_time` mit Default `0.3` filtert in
der Praxis genau die Züge mit `time_spent == 0.0`, also "Zug ist innerhalb
derselben Sekundenmarke gespielt worden" (Buchzüge, Premoves). Werte zwischen
`0.0` und `1.0` als Schwelle sind äquivalent — Zehntel kommen aus den PGNs
nicht heraus.

Inkrement-Zeitkontrollen (z.B. `300+1`) erzeugen negative `time_spent`-Werte
bei sofortigen Zügen (`last - curr = 300 - 301 = -1`); die werden vom
bestehenden `<`-Vergleich korrekt gefiltert. **Dort ist nichts zu reparieren.**

## Was die Änderung tut

Statt einer pauschalen Schwelle eine **pro Zeitkontrolle** (bullet/blitz/rapid/
classical), abgeleitet aus dem `[TimeControl "base+inc"]`-Tag über
`est = base + 40·inc`:

| Klasse | est-Bereich | Default-Schwelle (s) |
|---|---|---|
| bullet | `< 180` | 0 |
| blitz | `180 ≤ est < 480` | 1 |
| rapid | `480 ≤ est < 1500` | 3 |
| classical | `≥ 1500` | 5 |

Defaults sind die **konservative** Variante — bestehende Stichproben werden so
nur leicht strenger gefiltert als heute (heute filtert effektiv "0s gespielt",
neu auch "1s in Blitz", "1–2s in Rapid", "1–4s in Classical"). Bullet bleibt
bei 0, weil 1s in 1+0/2+1 echte Überlegung ist.

Schwellen sind `int` typisiert — Zehntel sind durch die PGN-Auflösung sinnlos,
das soll im `--help` direkt sichtbar sein.

## Skizze

```python
# tools/analyze_blunders.py — argparse-Block (nahe Zeile 580)

p.add_argument("--min-movetime-bullet",     type=int, default=0)
p.add_argument("--min-movetime-blitz",      type=int, default=1)
p.add_argument("--min-movetime-rapid",      type=int, default=3)
p.add_argument("--min-movetime-classical",  type=int, default=5)
p.add_argument("--min-movetime",            type=float, default=0.3,
               help="Fallback, wenn TimeControl-Tag fehlt/unparsbar.")
```

```python
# Neue Helfer

def classify_time_control(tc_tag: str) -> str | None:
    """Lichess-Klassifikation per est = base + 40 * inc.
    Gibt 'bullet'|'blitz'|'rapid'|'classical' oder None bei '-'/'*'/leer."""
    if not tc_tag or tc_tag in ("-", "*"):
        return None
    try:
        base, inc = tc_tag.split("+", 1)
        est = int(base) + 40 * int(inc)
    except ValueError:
        return None
    if est < 180:   return "bullet"
    if est < 480:   return "blitz"
    if est < 1500:  return "rapid"
    return "classical"

def threshold_for_game(game, args) -> float:
    klass = classify_time_control(game.headers.get("TimeControl", ""))
    if klass is None:
        return args.min_movetime          # alter Pfad
    return float(getattr(args, f"min_movetime_{klass}"))
```

```python
# In analyze_game(...): min_move_time-Parameter durch effective_threshold
# (aus threshold_for_game) ersetzen. Skip-Meldung mit Klasse + Schwelle:

print(
    f"  ({skipped_fast} move(s) skipped in {game_id} — {klass}, threshold {thr}s)",
    file=sys.stderr,
)
```

## Edge-Cases (`[TimeControl]`-Tag)

- `300+1`, `60+0` etc. → parsen, klassifizieren.
- `-` (Korrespondenz), `*`, leer, fehlend → `classify_time_control` gibt
  `None`, Fallback auf `--min-movetime` (heutiger Pfad bleibt funktional).
- Andere unparsbare Werte (z.B. `15/40+30/60` o.ä.) → ebenfalls `None`,
  Fallback. **Kein Hard-Error**, sonst bricht die Auswertung an exotischen
  PGNs ab.

## Reihenfolge / Zeitpunkt

**Nicht zusammen** mit der Engine-Anpassung vom 28.04.2026 umsetzen — sonst
sind die Vergleichszahlen aus der CLAUDE.md-Roadmap (Endgame 0.60,
missed_mate 0.075 …) nicht mehr direkt mit der nächsten Auswertung
vergleichbar.

Plan:

1. **28.04.2026 (erledigt):** Engine-Commit `77334e7` (Schach-Extension
   phase-abhängig).
2. **01.05.2026 (erledigt, 162-Partien-Auswertung):** Auswertung mit dem
   **bisherigen** Tool gelaufen → Engine-Änderung hat gewirkt
   (`docs/chesstrax-analyse-28.04.2026.md` bzw. CLAUDE.md-Roadmap-Eintrag).
3. **02.05.2026 (erledigt):** Tool-Umbau wie in dieser Skizze umgesetzt.
   `--min-movetime` bleibt als float-Fallback (Default `0.3`) für Partien
   ohne `[TimeControl]`-Tag erhalten; per-Klasse-Schalter sind `int` und
   greifen automatisch, sobald der Tag parsbar ist.

## Umsetzungs-Notizen (Abweichungen / Klarstellungen)

- `analyze_game()` bekam zusätzlich `tc_class: str | None`, damit die
  Skip-Meldung Klasse + Schwelle zeigen kann. Bei `tc_class is None`
  bleibt der alte Meldungstext erhalten.
- `threshold_for_game()` gibt `(float, str | None)` zurück. Der Aufrufer
  in `main()` ruft sie pro Spiel auf, weil der `[TimeControl]`-Tag erst
  beim Iterieren über die Partien bekannt ist.
- Default-`--min-movetime` blieb bei `0.3` (statt auf `1` zu gehen) —
  das ist nur noch Fallback-Pfad und betrifft Korrespondenz/`*`-Partien;
  da gibt es keinen Anlass, restriktiver zu sein.
