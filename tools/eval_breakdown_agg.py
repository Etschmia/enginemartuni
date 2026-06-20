#!/usr/bin/env python3
"""Aggregierter Eval-Breakdown ueber die positionellen Survivors.

Fuer jede After-Stellung (die Martuni gewaehlt hat): Martuni-Eval-Breakdown
parsen, auf MOVER-Sicht drehen, ueber alle Stellungen mitteln. Zeigt, welcher
Eval-Term systematisch zu Martunis Gunsten ausschlaegt, obwohl die Stellung
laut d26 schlecht ist (deep_after < 0).
"""
import json, subprocess, re
import chess
from collections import defaultdict

MART = "./target/release/martuni"
r = json.load(open("nomotif_verify.json"))
pos = [x for x in r if x["deep_loss"] >= 150 and abs(x["deep_after"]) < 5000]

def mart_eval(fen):
    out = subprocess.run([MART], input=f"position fen {fen}\neval\nquit\n",
                         capture_output=True, text=True, timeout=20).stdout
    comps = {}
    total = None
    for line in out.splitlines():
        m = re.search(r"info string\s+(\S.*?)\s+W=\s*(-?\d+)\s+B=\s*(-?\d+)\s+Diff=\s*(-?\d+)", line)
        if m:
            comps[m.group(1).strip()] = int(m.group(4))
        m2 = re.search(r"info string\s+(pst_tapered|mobility_tapered|rook_trapped_tapered|king_activity_eg|king_passed_synergy|pawn_eg_guard_taper|imbalance)\s+(-?\d+)", line)
        if m2:
            comps[m2.group(1)] = int(m2.group(2))
        m3 = re.search(r"info string total\s+(-?\d+)\s*cp", line)
        if m3:
            total = int(m3.group(1))
    return comps, total

agg = defaultdict(float)
n = 0
rows = []
for x in pos:
    board = chess.Board(x["fen"])
    mover = board.turn  # Martuni = mover (white=True)
    try:
        board.push(board.parse_san(x["mv"]))
    except Exception:
        continue
    fen_after = board.fen()
    comps, total = mart_eval(fen_after)
    if total is None:
        continue
    sign = 1 if mover == chess.WHITE else -1   # flip Diff (white-pov) to mover-pov
    mtotal = sign * total
    n += 1
    for k, v in comps.items():
        agg[k] += sign * v
    rows.append((x["mv"], mtotal, x["deep_after"], mtotal - x["deep_after"]))

print(f"=== Aggregierter Breakdown ueber {n} positionelle Survivors (Mover-Sicht) ===\n")
print(f"{'Komponente':24} {'Ø Mover-cp':>10}")
for k, v in sorted(agg.items(), key=lambda kv: -abs(kv[1])):
    print(f"  {k:22} {v/n:>+8.1f}")
import statistics
mt = [a for _, a, _, _ in rows]
da = [b for _, _, b, _ in rows]
gp = [g for _, _, _, g in rows]
print(f"\n  Martuni-total Ø {statistics.mean(mt):+.0f}  vs  d26-after Ø {statistics.mean(da):+.0f}  -> Gap Ø {statistics.mean(gp):+.0f} cp")
print(f"  (Martuni statische Eval ist im Schnitt {statistics.mean(gp):+.0f}cp zu optimistisch ggue. d26)")
