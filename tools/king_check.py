#!/usr/bin/env python3
"""Feuert Martunis bestehender king_safety-Term in den king_danger-Stellungen?
Wenn er ~0/positiv ist, obwohl >=2 Gegner den Koenig bedraengen → echte Luecke."""
import json, subprocess, re
import chess
ROOT="/home/librechat/enginemartuni"; MART=f"{ROOT}/target/release/martuni"
d=json.load(open(f"{ROOT}/analyse-04.06.2026.json"))
mg=[x for x in d["blunders"] if x["phase"]=="middlegame" and not x["motifs"]]

def king_danger(board, defender):
    ksq=board.king(defender)
    if ksq is None: return 0
    zone=set([ksq])|set(chess.SquareSet(chess.BB_KING_ATTACKS[ksq]))
    a=set()
    for sq in zone: a|=set(board.attackers(not defender,sq))
    return len(a)

def mart_ks(fen, mover):
    out=subprocess.run([MART],input=f"position fen {fen}\neval\nquit\n",
        capture_output=True,text=True,timeout=20).stdout
    m=re.search(r"king_safety\s+W=\s*(-?\d+)\s+B=\s*(-?\d+)",out)
    if not m: return None
    w,b=int(m.group(1)),int(m.group(2))
    return w-b if mover==chess.WHITE else b-w   # mover-Sicht (negativ=Martuni-Koenig in Gefahr)

vals=[]
for x in mg:
    b=chess.Board(x["fen_before"]); mover=b.turn
    try: b.push(b.parse_san(x["move_san"]))
    except Exception: continue
    kd=king_danger(b,mover)
    if kd>=2:
        ks=mart_ks(b.fen(),mover)
        if ks is not None: vals.append(ks)
import statistics
print(f"king_danger>=2 Stellungen: {len(vals)}")
print(f"Martunis king_safety (mover-Sicht): median {round(statistics.median(vals))}, min {min(vals)}, max {max(vals)}")
print(f"  davon >=0 (Term sieht KEINE Gefahr): {sum(1 for v in vals if v>=0)} / {len(vals)}")
print(f"  davon <=-20 (Term sieht Gefahr):     {sum(1 for v in vals if v<=-20)} / {len(vals)}")
