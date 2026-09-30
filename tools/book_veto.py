#!/usr/bin/env python3
"""Schritt 3 des Eigenbuchs (docs/eigenes-eroeffnungsbuch.md, Abschnitt 8):
Stockfish-Veto ueber Martunis Kandidatenzuege.

Stockfish schlaegt KEINE Zuege vor, er streicht nur (Tobias-Entscheid
30.09.2026). Pro Buchstellung eine Fairy-Stockfish-Suche mit MultiPV ueber
Martunis Kandidaten plus Stockfishs freien Bestzug (`searchmoves`); alle
Scores stammen aus derselben Suche. Ein Kandidat faellt raus, wenn er mehr als
--max-loss cp hinter dem Stockfish-Bestzug liegt.

Aufruf: python3 tools/book_veto.py 3check|koth [--depth 18] [--max-loss 60]
Eingabe: book_work/<variante>_analysis.json (tools/book_analyze.py)
Ausgabe: book_work/<variante>_veto.json
"""
import argparse, json, os, subprocess

ENGINE = os.path.expanduser("~/tools/fairy-stockfish")
UCI_NAME = {"3check": "3check", "koth": "kingofthehill"}
MATE = 30000


class Fairy:
    def __init__(self, variant, hash_mb, threads):
        self.p = subprocess.Popen([ENGINE], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1)
        for c in ("uci", f"setoption name Hash value {hash_mb}", f"setoption name Threads value {threads}",
                  f"setoption name UCI_Variant value {UCI_NAME[variant]}"):
            self.send(c)
        self.sync()

    def send(self, s):
        self.p.stdin.write(s + "\n")
        self.p.stdin.flush()

    def sync(self):
        self.send("isready")
        while "readyok" not in self.p.stdout.readline():
            pass

    def analyse(self, moves, depth, multipv, searchmoves=None):
        """Liefert {zug: score} aus Sicht der Seite am Zug (letzte Tiefe)."""
        self.send(f"setoption name MultiPV value {multipv}")
        self.send("position startpos" + (" moves " + " ".join(moves) if moves else ""))
        cmd = f"go depth {depth}"
        if searchmoves:
            cmd += " searchmoves " + " ".join(searchmoves)
        self.send(cmd)
        res = {}
        while True:
            t = self.p.stdout.readline().split()
            if not t:
                continue
            if t[0] == "bestmove":
                return res
            if t[0] == "info" and "pv" in t and "score" in t and "multipv" in t:
                i = t.index("score")
                if t[i + 1] == "cp":
                    s = int(t[i + 2])
                else:
                    n = int(t[i + 2])
                    s = MATE - abs(n) if n > 0 else -(MATE - abs(n))
                if "lowerbound" in t or "upperbound" in t:
                    continue
                res[t[t.index("pv") + 1]] = s

    def quit(self):
        self.send("quit")
        self.p.wait(timeout=5)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("variant", choices=UCI_NAME)
    ap.add_argument("--depth", type=int, default=18)
    ap.add_argument("--max-loss", type=int, default=60)
    ap.add_argument("--hash", type=int, default=128)
    ap.add_argument("--threads", type=int, default=1)
    a = ap.parse_args()

    src = json.load(open(f"book_work/{a.variant}_analysis.json"))
    fy = Fairy(a.variant, a.hash, a.threads)
    out = []
    for i, r in enumerate(src["results"], 1):
        cands = list(r["candidates"])
        fy.send("ucinewgame")
        # Erst frei suchen, um Stockfishs Bestzug zu kennen; verglichen wird
        # aber nur innerhalb EINER MultiPV-Suche (Kandidaten + Bestzug), weil
        # Scores aus getrennten Suchen nicht vergleichbar sind.
        free = fy.analyse(r["moves_from_start"], a.depth, 1)
        free_best = max(free, key=free.get) if free else None
        pool = list(dict.fromkeys(cands + ([free_best] if free_best else [])))
        sc = fy.analyse(r["moves_from_start"], a.depth, len(pool), pool)
        sf_best_move, sf_best = max(sc.items(), key=lambda kv: kv[1]) if sc else (None, None)
        verdict = {}
        for mv in cands:
            s = sc.get(mv)
            loss = None if s is None or sf_best is None else max(0, sf_best - s)
            verdict[mv] = {"sf_score": s, "sf_loss": loss,
                           "veto": loss is None or loss > a.max_loss}
        out.append({"fen": r["fen"], "sf_best": {"move": sf_best_move, "score": sf_best},
                    "verdict": verdict})
        kept = sum(not v["veto"] for v in verdict.values())
        print(f"[{i}/{len(src['results'])}] ply {r['ply']:2d} SF {sf_best_move} {sf_best} "
              f"| {kept}/{len(cands)} Kandidaten bleiben", flush=True)
        json.dump({"variant": a.variant, "depth": a.depth, "max_loss": a.max_loss, "results": out},
                  open(f"book_work/{a.variant}_veto.json", "w"), indent=1)
    fy.quit()


if __name__ == "__main__":
    main()
