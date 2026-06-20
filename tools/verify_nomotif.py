#!/usr/bin/env python3
"""Tief-Verifikation der motivlosen Blunder (loss 150-299, kein Motiv).

Der Analyzer misst mit SF @ 0.3s (~d17). Ein 150-200cp "Verlust" in dieser
Groessenordnung kann tief kippen (Snapshot-Trap, vgl. feedback_sf_d17_snapshot).
Re-evaluiert eine geschichtete Stichprobe zeit-gebunden (movetime, ~6x tiefer
als Baseline) und schreibt INKREMENTELL (Timeout rettet Teildaten).
"""
import json, sys
import chess, chess.engine

SF = "/usr/games/stockfish"
MOVETIME = 2.0
THREADS = 4
HASH = 512
SAMPLE = 50
OUT = "nomotif_verify.json"

d = json.load(open("analyse-04.06.2026.json"))
nm = [x for x in d["blunders"] if not x["motifs"]]
nm.sort(key=lambda x: (x["phase"], x["loss_cp"]))
k = max(1, len(nm)//SAMPLE)
sample = nm[::k][:SAMPLE]

eng = chess.engine.SimpleEngine.popen_uci(SF)
eng.configure({"Threads": THREADS, "Hash": HASH})
limit = chess.engine.Limit(time=MOVETIME)

def cp(score, pov):
    s = score.pov(pov)
    if s.is_mate():
        m = s.mate()
        return (100000 - abs(m)*10) if m > 0 else (-100000 + abs(m)*10)
    return s.score()

out = []
for i, x in enumerate(sample):
    board = chess.Board(x["fen_before"])
    mover = board.turn
    info_b = eng.analyse(board, limit)
    deep_before = cp(info_b["score"], mover)
    best = board.san(info_b["pv"][0]) if info_b.get("pv") else None
    db = info_b.get("depth")
    try:
        mv = board.parse_san(x["move_san"])
    except Exception:
        continue
    board.push(mv)
    info_a = eng.analyse(board, limit)
    deep_after = cp(info_a["score"], mover)
    deep_loss = deep_before - deep_after
    out.append({
        "game": x["game_id"][:40], "mv": x["move_san"], "phase": x["phase"],
        "shallow_loss": x["loss_cp"], "deep_loss": deep_loss,
        "deep_before": deep_before, "deep_after": deep_after, "depth": db,
        "martuni_eval": x["martuni_eval_cp"], "sf_best": best,
        "best_san": x["best_move_san"], "fen": x["fen_before"],
    })
    json.dump(out, open(OUT, "w"), indent=1)   # incremental: survive timeout
    print(f"[{i+1}/{len(sample)}] d{db} {x['phase'][:3]} {x['move_san']:6} shallow{x['shallow_loss'] if 'shallow_loss' in x else x['loss_cp']:4} -> deep{deep_loss:5} (best {best})", flush=True)

eng.quit()
print(f"\nWrote {len(out)} verified positions to {OUT}", flush=True)
