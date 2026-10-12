#!/usr/bin/env python3
"""Phase 2 des Eigenbuchs (docs/eigenes-eroeffnungsbuch.md, Abschnitt 11):
Stockfish-Veto ueber Martunis live gespielte Standard-Buchzuege.

Wie book_veto.py fuer die Varianten: Stockfish schlaegt KEINE Zuege vor, er
streicht nur (Tobias-Entscheid 30.09.2026). Kandidaten sind hier die Zuege,
die Martuni live aus den fremden Buechern gespielt hat
(book_std_collect.py), nicht eine eigene Martuni-Analyse.

Pro Stellung zuerst eine freie Suche (MultiPV 1) fuer Stockfishs Bestzug.
Ist der einzige Kandidat genau dieser Zug, ist der Verlust 0 und es braucht
keine zweite Suche (der haeufigste Fall). Sonst eine MultiPV-Suche ueber
Kandidaten + Bestzug (`searchmoves`), damit alle Scores aus EINER Suche
stammen. sf_loss wird gespeichert; die Schwelle (--max-loss, Default 60 cp)
wird erst in book_std_weights.py angewendet und laesst sich ohne Neurechnen
aendern.

Inkrementell und unterbrechbar: Stellungen mit unveraenderten Kandidaten
werden uebersprungen. Reihenfolge = Haeufigkeit (book_std_positions.json ist
danach sortiert), ein abgebrochener Lauf deckt also die wichtigen Stellungen
zuerst ab.

Aufruf: nice -n 19 python3 tools/book_std_veto.py [--depth 18] [--limit N]
Eingabe: book_work/std_positions.json
Ausgabe: book_work/std_veto.json  {"depth", "results": [{"fen", "sf_best": {move, score},
          "verdict": {uci: {"sf_score", "sf_loss"}}}]}
"""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from book_veto import Fairy  # noqa: E402  (UCI-Wrapper, mit variant=None = Standard)

STOCKFISH = "/usr/games/stockfish"
OUT = "book_work/std_veto.json"


def save(depth, results):
    json.dump({"depth": depth, "results": list(results.values())}, open(OUT + ".tmp", "w"))
    os.replace(OUT + ".tmp", OUT)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--depth", type=int, default=18)
    ap.add_argument("--hash", type=int, default=128)
    ap.add_argument("--threads", type=int, default=1)
    ap.add_argument("--limit", type=int, default=None, help="nur die N haeufigsten Stellungen")
    a = ap.parse_args()

    src = json.load(open("book_work/std_positions.json"))["positions"]
    if a.limit:
        src = src[:a.limit]
    results = {}
    if os.path.exists(OUT):
        results = {r["fen"]: r for r in json.load(open(OUT))["results"]}

    sf = Fairy(None, a.hash, a.threads, engine=STOCKFISH)
    t0, done = time.time(), 0
    for i, p in enumerate(src, 1):
        cands = sorted(p["played"])
        old = results.get(p["fen"])
        if old and sorted(old["verdict"]) == cands:
            continue
        sf.send("ucinewgame")
        free = sf.analyse(p["moves_from_start"], a.depth, 1)
        best = max(free, key=free.get) if free else None
        if best is not None and cands == [best]:
            sc = free
        else:
            pool = list(dict.fromkeys(cands + ([best] if best else [])))
            sc = sf.analyse(p["moves_from_start"], a.depth, len(pool), pool)
        best_move, best_score = max(sc.items(), key=lambda kv: kv[1]) if sc else (None, None)
        verdict = {}
        for mv in cands:
            s = sc.get(mv)
            loss = None if s is None or best_score is None else max(0, best_score - s)
            verdict[mv] = {"sf_score": s, "sf_loss": loss}
        results[p["fen"]] = {"fen": p["fen"], "sf_best": {"move": best_move, "score": best_score},
                             "verdict": verdict}
        done += 1
        if done % 50 == 0:
            save(a.depth, results)
            rate = (time.time() - t0) / done
            print(f"[{i}/{len(src)}] {done} gerechnet, {rate:.2f} s/Stellung, "
                  f"Rest ~{rate * (len(src) - i) / 3600:.1f} h", flush=True)
    save(a.depth, results)
    sf.quit()
    print(f"VETO-FERTIG: {len(results)} Stellungen in {OUT}", flush=True)


if __name__ == "__main__":
    main()
