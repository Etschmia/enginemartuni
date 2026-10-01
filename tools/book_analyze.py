#!/usr/bin/env python3
"""Schritt 2 des Eigenbuchs (docs/eigenes-eroeffnungsbuch.md, Abschnitt 8):
Martunis eigene Tiefenanalyse der gesammelten Fruehstellungen.

Martuni hat kein MultiPV und kein searchmoves. Damit die Engine unveraendert
bleibt, werden Kandidatenzuege ueber ihre Folgestellung bewertet:

  1. Wurzelsuche (--root-ms): Martunis natuerlicher Zug und Score.
  2. Kurz-Scan aller legalen Zuege (--scan-ms je Folgestellung).
  3. Tiefsuche (--deep-ms) fuer die besten --top Zuege aus dem Scan, den
     Wurzelzug und alle live gespielten Zuege.

Scores sind aus Sicht der Seite, die in der Buchstellung am Zug ist
(Folgestellung-Score negiert). Matt wird auf +-30000 abgebildet.

Fortsetzbar: bereits analysierte Stellungen werden uebersprungen, das
Ergebnis wird nach jeder Stellung geschrieben.

Nachlauf (--refine, Tobias-Entscheid 01.10.2026): nur Stellungen, in denen
nach dem Veto (book_work/<variante>_veto.json, Schwelle wie in
book_veto.py) weniger als zwei Zuege uebrig sind. Dort wird neu gescannt und
die besten --top Zuege (Default im Nachlauf 8) werden zusaetzlich tief
gerechnet; schon vorhandene Kandidaten und die Wurzelsuche bleiben.

Rechenlast: EIN Engine-Prozess (Server hat 2 Kerne, der Lichess-Bot braucht
einen). Mit `nice -n 10` starten.

Aufruf: python3 tools/book_analyze.py 3check|koth
Eingabe: book_work/<variante>_positions.json (tools/book_collect.py)
Ausgabe: book_work/<variante>_analysis.json
"""
import argparse, json, os, subprocess, sys, time
import chess, chess.variant

ENGINE = os.path.expanduser("~/enginemartuni/target/release/martuni")
UCI_NAME = {"3check": "3check", "koth": "kingofthehill"}
BOARD = {"3check": chess.variant.ThreeCheckBoard, "koth": chess.variant.KingOfTheHillBoard}
MATE = 30000


class Engine:
    def __init__(self, variant, hash_mb):
        self.p = subprocess.Popen([ENGINE], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1)
        self.send("uci")
        self.send(f"setoption name Hash value {hash_mb}")
        self.send(f"setoption name UCI_Variant value {UCI_NAME[variant]}")
        self.sync()

    def send(self, s):
        self.p.stdin.write(s + "\n")
        self.p.stdin.flush()

    def sync(self):
        self.send("isready")
        while "readyok" not in self.p.stdout.readline():
            pass

    def search(self, moves, ms):
        """Sucht `startpos moves ...`; liefert (bestmove, score, depth)."""
        self.send("position startpos" + (" moves " + " ".join(moves) if moves else ""))
        self.send(f"go movetime {ms}")
        score, depth = None, 0
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("Engine beendet")
            t = line.split()
            if t and t[0] == "info" and "score" in t:
                i = t.index("score")
                if t[i + 1] == "cp":
                    score = int(t[i + 2])
                elif t[i + 1] == "mate":
                    n = int(t[i + 2])
                    score = MATE - abs(n) if n > 0 else -(MATE - abs(n))
                if "depth" in t:
                    depth = int(t[t.index("depth") + 1])
            elif t and t[0] == "bestmove":
                return t[1], score, depth

    def newgame(self):
        self.send("ucinewgame")
        self.sync()

    def quit(self):
        try:
            self.send("quit")
            self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


def child_score(eng, board, moves, mv, ms):
    """Score des Zugs mv aus Sicht der Seite am Zug in `board`."""
    b = board.copy()
    b.push(chess.Move.from_uci(mv))
    if b.is_game_over():
        r = b.result()
        if r == "1/2-1/2":
            return 0, 0
        mover_wins = (r == "1-0") == (board.turn == chess.WHITE)
        return (MATE if mover_wins else -MATE), 0
    _, s, d = eng.search(moves + [mv], ms)
    return (-s if s is not None else None), d


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("variant", choices=UCI_NAME)
    ap.add_argument("--root-ms", type=int, default=5000)
    ap.add_argument("--scan-ms", type=int, default=150)
    ap.add_argument("--deep-ms", type=int, default=6000)
    ap.add_argument("--top", type=int, default=None, help="Default 4, im Nachlauf 8")
    ap.add_argument("--hash", type=int, default=64)
    ap.add_argument("--refine", action="store_true", help="Nachlauf fuer Stellungen mit < 2 Zuegen")
    ap.add_argument("--max-loss", type=int, default=None, help="Veto-Schwelle fuer --refine")
    a = ap.parse_args()
    if a.top is None:
        a.top = 8 if a.refine else 4

    src = json.load(open(f"book_work/{a.variant}_positions.json"))
    out_path = f"book_work/{a.variant}_analysis.json"
    done = {}
    if os.path.exists(out_path):
        done = {r["fen"]: r for r in json.load(open(out_path))["results"]}
    settings = {k: getattr(a, k.replace("-", "_")) for k in ("root_ms", "scan_ms", "deep_ms", "top")}

    eng = Engine(a.variant, a.hash)
    if a.refine:
        from book_veto import MAX_LOSS, kept_moves
        max_loss = a.max_loss if a.max_loss is not None else MAX_LOSS[a.variant]
        veto = json.load(open(f"book_work/{a.variant}_veto.json"))["results"]
        thin = {r["fen"] for r in veto if len(kept_moves(r, max_loss)) < 2}
        todo = [p for p in src["positions"] if p["fen"] in thin and p["fen"] in done]
        print(f"Nachlauf: {len(todo)} Stellungen mit < 2 Zuegen bei {max_loss} cp, Top {a.top}", flush=True)
    else:
        todo = [p for p in src["positions"] if p["fen"] not in done]
    print(f"{a.variant}: {len(done)} fertig, {len(todo)} offen", flush=True)
    t0 = time.time()
    for i, p in enumerate(todo, 1):
        moves = p["moves_from_start"]
        board = BOARD[a.variant]()
        for m in moves:
            board.push_uci(m)
        eng.newgame()
        old = done.get(p["fen"]) if a.refine else None
        if old:
            r0 = old["root"]
            root_best, root_score, root_depth = r0["move"], r0["score"], r0["depth"]
        else:
            root_best, root_score, root_depth = eng.search(moves, a.root_ms)

        scan = {}
        for mv in board.legal_moves:
            s, _ = child_score(eng, board, moves, mv.uci(), a.scan_ms)
            scan[mv.uci()] = s
        ranked = sorted((m for m in scan if scan[m] is not None), key=lambda m: -scan[m])
        cands = list(dict.fromkeys(ranked[: a.top] + [root_best] + list(p["played"])))

        deep = dict(old["candidates"]) if old else {}
        for mv in cands:
            if mv not in scan or mv in deep:  # illegal/alt bzw. schon gerechnet
                continue
            s, d = child_score(eng, board, moves, mv, a.deep_ms)
            deep[mv] = {"score": s, "depth": d, "scan": scan[mv],
                        "san": board.san(chess.Move.from_uci(mv)),
                        "live": p["played"].get(mv)}
        done[p["fen"]] = {
            "fen": p["fen"], "ply": p["ply"], "moves_from_start": moves, "count": p["count"],
            "live_score": p["score"], "root": {"move": root_best, "score": root_score, "depth": root_depth},
            "candidates": deep,
        }
        if old:
            done[p["fen"]]["refined_top"] = a.top
        json.dump({"variant": a.variant, "settings": settings, "results": list(done.values())},
                  open(out_path + ".tmp", "w"), indent=1)
        os.replace(out_path + ".tmp", out_path)
        best = max(deep.items(), key=lambda kv: kv[1]["score"] if kv[1]["score"] is not None else -1e9)
        el = time.time() - t0
        print(f"[{i}/{len(todo)}] ply {p['ply']:2d} n={p['count']:3d} wurzel {root_best} {root_score} "
              f"| bester {best[1]['san']} {best[1]['score']} | {len(deep)} Kandidaten | "
              f"{el/60:.1f} min, Rest ~{el/i*(len(todo)-i)/60:.0f} min", flush=True)
    eng.quit()
    print("fertig", flush=True)


if __name__ == "__main__":
    main()
