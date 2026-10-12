#!/usr/bin/env python3
"""Phase 2 des Eigenbuchs (docs/eigenes-eroeffnungsbuch.md, Abschnitt 11):
Standardbuch aus Martunis live gespielten Buchzuegen zusammenstellen.

Tobias-Entscheide 12.10.2026:
  * Alle live gespielten Buchzuege kommen rein, auch Stellungen mit nur einer
    Partie oder nur einem Zug (die >= 2-Zuege-Regel der Varianten gilt hier
    nicht, sonst wuerde das Buch flach).
  * Stockfish-Veto: Zug faellt raus, wenn er mehr als MAX_LOSS cp hinter
    Stockfishs Bestzug liegt (book_std_veto.py). Bleibt in einer Stellung
    nichts uebrig, ist sie nicht im Buch und Martuni rechnet dort selbst.
  * Score-Filter: Zug faellt raus, wenn er mindestens SCORE_MIN_N (10) Partien
    hat und sein geschrumpfter Live-Score
        (Punkte - Erwartet) / (n + k),  k = 4
    unter SCORE_MIN (-5 %) liegt, aber nur, wenn in der Stellung ein anderer
    Zug uebrig bleibt. Sind alle uebrigen Zuege schlecht, bleibt der am
    wenigsten schlechte. Das Mindest-n kam nach dem ersten Probelauf dazu:
    Mit k = 4 reicht sonst EINE verlorene Partie bei gleicher Elo (-0,5/5 =
    -10 %), das waeren 984 Streichungen, 758 davon mit 1-4 Partien (Zufall).
  * Harter Wechsel: das Buch ersetzt gm2001/komodo/rodent/martuni_patches
    komplett (BOOK_FILES=martuni_std.bin).

Gewichte (Tobias-Entscheid 12.10.2026). Anders als bei den Varianten gibt es
keine Martuni-Analyse (Q_M). Ohne sie waeren die Gewichte fast gleichverteilt
und Nebenlinien mit 2 Partien kaemen so oft wie die Hauptlinie. Basis ist
deshalb die Live-Haeufigkeit H, im Kern also die Gewichtung der alten Buecher:
    roh  = H * Q_SF * L
    H    = n_Zug / n_Stellung              wie oft Martuni den Zug live spielte
    Q_SF = exp(-SF-Verlust / T),  T = MAX_LOSS
    L    = 2 * (0.5 + (Punkte - Erwartet) / (n + k)),  begrenzt auf [0.05, 2]
    Anteil = roh / Summe, jeder Zug mindestens 2 %, Gewicht = Anteil * 1000

Aufruf: python3 tools/book_std_weights.py [--max-loss 60] [--allow-missing]
  --allow-missing: Stellungen ohne Veto-Ergebnis auslassen statt abzubrechen
                   (fuer Zwischenstaende, waehrend book_std_veto.py noch laeuft)
Eingabe: book_work/std_positions.json, book_work/std_veto.json
Ausgabe: book_work/std_book.json (alle Zwischenwerte), book_work/std_book.txt
         (Eingabe fuer bookbuild std)
"""
import argparse
import json
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from book_weights import K, SCALE, apply_min_share, live_factor, write_txt  # noqa: E402

MAX_LOSS = 60
SCORE_MIN = -0.05
SCORE_MIN_N = 10
MIN_SHARE = 0.02


def shrunk(st):
    """Geschrumpfter Live-Score: (Punkte - Erwartet) / (n + k)."""
    return (st["score"] - st["exp"]) / (st["n"] + K)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--max-loss", type=int, default=MAX_LOSS)
    ap.add_argument("--allow-missing", action="store_true")
    a = ap.parse_args()
    T = a.max_loss

    positions = json.load(open("book_work/std_positions.json"))["positions"]
    veto = {r["fen"]: r for r in json.load(open("book_work/std_veto.json"))["results"]}

    out = []
    stat = {"missing": 0, "veto": 0, "score": 0, "leer": 0}
    for p in positions:
        r = veto.get(p["fen"])
        if r is None or set(r["verdict"]) != set(p["played"]):
            if not a.allow_missing:
                sys.exit(f"FEHLER: kein aktuelles Veto fuer {p['fen']} (book_std_veto.py laufen lassen)")
            stat["missing"] += 1
            continue
        # 1. Stockfish-Veto
        kept = [m for m, v in r["verdict"].items() if v["sf_loss"] is not None and v["sf_loss"] <= T]
        stat["veto"] += len(p["played"]) - len(kept)
        if not kept:
            stat["leer"] += 1
            continue
        # 2. Score-Filter, nur wenn eine Alternative bleibt
        good = [m for m in kept
                if p["played"][m]["n"] < SCORE_MIN_N or shrunk(p["played"][m]) >= SCORE_MIN]
        if not good:
            good = [max(kept, key=lambda m: shrunk(p["played"][m]))]
        stat["score"] += len(kept) - len(good)
        # 3. Gewichte
        n_pos = sum(v["n"] for v in p["played"].values())
        rows = {}
        for m in good:
            st = p["played"][m]
            loss = r["verdict"][m]["sf_loss"]
            q_sf = math.exp(-loss / T)
            L = live_factor(st)
            h = st["n"] / n_pos
            rows[m] = {"uci": m, "sf_loss": loss, "h": round(h, 4), "q_sf": round(q_sf, 3), "live": st,
                       "shrunk": round(shrunk(st), 4), "L": round(L, 3), "raw": h * q_sf * L}
        total = sum(x["raw"] for x in rows.values())
        share = apply_min_share({m: x["raw"] / total for m, x in rows.items()}, MIN_SHARE)
        for m, x in rows.items():
            x["share"] = round(share[m], 4)
            x["weight"] = max(1, round(share[m] * SCALE))
            x["raw"] = round(x["raw"], 4)
        out.append({"fen": p["fen"], "ply": p["ply"], "moves_from_start": p["moves_from_start"],
                    "moves": sorted(rows.values(), key=lambda x: -x["weight"])})

    out.sort(key=lambda p: (p["ply"], p["fen"]))
    json.dump({"variant": "std", "formula": {"T": T, "k": K, "score_min": SCORE_MIN, "score_min_n": SCORE_MIN_N, "scale": SCALE,
                                             "min_share": MIN_SHARE, "raw": "H * Q_SF * L",
                                             "live": "elo-korrigiert"},
               "stats": stat, "positions": out}, open("book_work/std_book.json", "w"), indent=1)
    write_txt("std", out, "book_work/std_book.txt")
    n_moves = sum(len(p["moves"]) for p in out)
    print(f"std: {len(out)} Buchstellungen, {n_moves} Zuege -> book_work/std_book.json + .txt | "
          f"gestrichen: Veto {stat['veto']}, Score {stat['score']}, Stellungen leer {stat['leer']}, "
          f"ohne Veto {stat['missing']}")


if __name__ == "__main__":
    main()
