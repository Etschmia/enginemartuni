#!/usr/bin/env python3
"""Selfplay-Match fuer Schachvarianten (python-chess, UCI, echte Uhr) — 22.09.2026.

fastchess kann nur standard/fischerandom; fuer Three-check, King of the Hill,
Horde und Racing Kings braucht es diesen Runner (laeuft in .venv, braucht
python-chess). Beispiel:

  .venv/bin/python3 tools/variant_match.py --variant 3check \\
      --a target/dev/release/martuni --b target/release/martuni-baseline \\
      --games 50 --tc 10+0.1 --concurrency 2 --out matches/3check_test

Optionen:
  --variant 3check|kingofthehill|horde|racingkings
  --a / --b   Engine-Binaries (A = neu, B = Baseline)
  --games N   Anzahl Paare (jedes Paar = 2 Partien, Farben gespiegelt)
  --tc 10+0.1 Basis+Inkrement in Sekunden
  --openings  EPD (UHO) fuer 3check/KotH; sonst zufaellige 4 Halbzuege
Ergebnis: W/D/L aus Sicht von A, Elo-Schaetzung mit 95%-CI, PGN je Partie.
"""
import argparse, chess, chess.engine, chess.pgn, chess.variant, math, os, random, sys, time
from multiprocessing import Pool

VARIANTS = {
    "3check": chess.variant.ThreeCheckBoard,
    "kingofthehill": chess.variant.KingOfTheHillBoard,
    "horde": chess.variant.HordeBoard,
    "racingkings": chess.variant.RacingKingsBoard,
}
MAX_PLIES = 400

def opening_board(variant, seed, openings):
    cls = VARIANTS[variant]
    rng = random.Random(seed)
    if openings and variant in ("3check", "kingofthehill"):
        fen = rng.choice(openings)
        return cls(fen)
    b = cls()
    for _ in range(4):
        moves = list(b.legal_moves)
        if not moves or b.is_game_over(): break
        b.push(rng.choice(moves))
    return b

def play_game(args):
    variant, seed, white_path, black_path, base_ms, inc_ms, openings, white_name, black_name = args
    board = opening_board(variant, seed, openings)
    engines = {}
    for name, path in ((white_name, white_path), (black_name, black_path)):
        if name not in engines:
            e = chess.engine.SimpleEngine.popen_uci(path)
            e.configure({"Hash": 64, "MoveOverhead": 0})
            engines[name] = e
    clocks = {chess.WHITE: base_ms, chess.BLACK: base_ms}
    names = {chess.WHITE: white_name, chess.BLACK: black_name}
    result, reason = None, ""
    try:
        while True:
            if board.is_variant_end() or board.is_game_over():
                result = board.result(); reason = "rules"; break
            if board.ply() - 8 > MAX_PLIES:
                result = "1/2-1/2"; reason = "maxplies"; break
            stm = board.turn
            eng = engines[names[stm]]
            lim = chess.engine.Limit(white_clock=clocks[chess.WHITE]/1000, black_clock=clocks[chess.BLACK]/1000,
                                     white_inc=inc_ms/1000, black_inc=inc_ms/1000)
            t0 = time.perf_counter()
            res = eng.play(board, lim)
            used = (time.perf_counter() - t0) * 1000
            clocks[stm] -= used
            if clocks[stm] < 0:
                result = "0-1" if stm == chess.WHITE else "1-0"; reason = "time"; break
            clocks[stm] += inc_ms
            if res.move is None:
                result = "0-1" if stm == chess.WHITE else "1-0"; reason = "nomove"; break
            board.push(res.move)
    finally:
        for e in engines.values():
            try: e.quit()
            except Exception: pass
    # PGN: setup() setzt FEN/SetUp aus der WURZEL (vor den Eroeffnungszuegen);
    # die zufaelligen Eroeffnungszuege stehen dann als normale Zuege drin.
    # (22.09.2026: ein manuell gesetzter FEN-Header mit der Stellung NACH der
    # Eroeffnung liess den SAN-Writer auf dem ersten Zug abstuerzen.)
    game = chess.pgn.Game()
    game.setup(board.root())
    game.headers["Variant"] = variant
    game.headers["White"] = white_name; game.headers["Black"] = black_name
    game.headers["Result"] = result; game.headers["Termination"] = reason
    node = game
    for mv in board.move_stack: node = node.add_variation(mv)
    return white_name, black_name, result, reason, str(game)

def elo_ci(w, d, l):
    n = w + d + l
    if n == 0: return 0, 0
    score = (w + 0.5*d) / n
    score = min(max(score, 1e-6), 1-1e-6)
    elo = -400 * math.log10(1/score - 1)
    var = (w*(1-score)**2 + d*(0.5-score)**2 + l*(0-score)**2) / n
    sd = math.sqrt(var / n)
    lo = -400*math.log10(1/min(max(score-1.96*sd,1e-6),1-1e-6) - 1)
    hi = -400*math.log10(1/min(max(score+1.96*sd,1e-6),1-1e-6) - 1)
    return elo, (hi-lo)/2

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--variant", required=True, choices=VARIANTS)
    ap.add_argument("--a", required=True); ap.add_argument("--b", required=True)
    ap.add_argument("--games", type=int, default=50, help="Paare (x2 Partien)")
    ap.add_argument("--tc", default="10+0.1")
    ap.add_argument("--concurrency", type=int, default=2)
    ap.add_argument("--openings", default="/home/librechat/tools/openings/UHO_Lichess_4852_v1.epd")
    ap.add_argument("--no-openings", action="store_true")
    ap.add_argument("--out", required=True)
    ap.add_argument("--seed", type=int, default=20260922)
    a = ap.parse_args()
    base, inc = a.tc.split("+"); base_ms, inc_ms = float(base)*1000, float(inc)*1000
    openings = None
    if not a.no_openings and os.path.exists(a.openings):
        with open(a.openings) as f:
            openings = [ln.split(";")[0].strip() for ln in f if ln.strip()]
        openings = [o + " 0 1" if len(o.split()) == 4 else o for o in openings]
    os.makedirs(a.out, exist_ok=True)
    jobs = []
    for i in range(a.games):
        seed = a.seed + i
        jobs.append((a.variant, seed, a.a, a.b, base_ms, inc_ms, openings, "A", "B"))
        jobs.append((a.variant, seed, a.b, a.a, base_ms, inc_ms, openings, "B", "A"))
    W = D = L = 0; reasons = {}
    t0 = time.time()
    with open(os.path.join(a.out, "games.pgn"), "w") as pgn, Pool(a.concurrency) as pool:
        for k, (wn, bn, result, reason, text) in enumerate(pool.imap_unordered(play_game, jobs), 1):
            pgn.write(text + "\n\n"); pgn.flush()
            reasons[reason] = reasons.get(reason, 0) + 1
            if result == "1/2-1/2": D += 1
            elif (result == "1-0") == (wn == "A"): W += 1
            else: L += 1
            if k % 10 == 0 or k == len(jobs):
                elo, ci = elo_ci(W, D, L)
                print(f"[{k}/{len(jobs)}] A: W{W} D{D} L{L}  score={(W+0.5*D)/(W+D+L)*100:.1f}%  Elo {elo:+.1f} ±{ci:.1f}  ({time.time()-t0:.0f}s)  {reasons}", flush=True)
    elo, ci = elo_ci(W, D, L)
    print(f"FINAL {a.variant} tc={a.tc}: A vs B  W{W} D{D} L{L}  score={(W+0.5*D)/(W+D+L)*100:.1f}%  Elo {elo:+.1f} ±{ci:.1f}  reasons={reasons}")

if __name__ == "__main__":
    main()
