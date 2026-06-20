#!/usr/bin/env python3
"""Outpost-Gate: Optimismus-Gap auf den 35 Stellungen mit aktivem Outpost-Bonus,
+ wie oft der Term ueberhaupt feuert (W/B). Zerstoerungsfrei (Temp-CWD)."""
import json, subprocess, re, os, tempfile, shutil
import chess

ROOT="/home/librechat/enginemartuni"; MART=f"{ROOT}/target/release/martuni"
BASE=open(f"{ROOT}/eval.toml").read()
def variant(mg,eg):
    t=re.sub(r"(?m)^knight_mg = 0", f"knight_mg = {mg}", BASE)
    t=re.sub(r"(?m)^knight_eg = 0", f"knight_eg = {eg}", t)
    return t
VARIANTS={"baseline":(0,0),"out_25_15":(25,15),"out_40_25":(40,25)}

r=json.load(open(f"{ROOT}/nomotif_verify.json"))
pos=[x for x in r if x["deep_loss"]>=150 and abs(x["deep_after"])<5000]
def probe(fen,cwd):
    out=subprocess.run([MART],input=f"position fen {fen}\neval\nquit\n",
        capture_output=True,text=True,timeout=20,cwd=cwd).stdout
    tot=re.search(r"info string total\s+(-?\d+)\s*cp",out)
    op=re.search(r"knight_outpost\s+W=\s*(-?\d+)\s+B=\s*(-?\d+)",out)
    return (int(tot.group(1)) if tot else None,
            (int(op.group(1)),int(op.group(2))) if op else (0,0))
items=[]
for x in pos:
    b=chess.Board(x["fen"]); mv=b.turn
    try: b.push(b.parse_san(x["mv"]))
    except Exception: continue
    items.append((b.fen(),mv,x["deep_after"]))

print(f"=== Outpost-Gate: {len(items)} Stellungen (d26-after Ø {round(sum(d for _,_,d in items)/len(items))}) ===\n")
print(f"{'Variante':12} {'Ø gap':>8} {'>150 rosy':>10} {'feuert(W|B mover-Sicht)':>26}")
for name,(mg,eg) in VARIANTS.items():
    d=tempfile.mkdtemp(); open(os.path.join(d,"eval.toml"),"w").write(variant(mg,eg))
    gaps=[]; fires_self=0; fires_opp=0
    for fen,mover,da in items:
        t,(w,bl)=probe(fen,d)
        if t is None: continue
        mt=t if mover==chess.WHITE else -t
        gaps.append(mt-da)
        # mover-Sicht: Martuni=mover. own outpost / opp outpost
        own = w if mover==chess.WHITE else bl
        opp = bl if mover==chess.WHITE else w
        if own>0: fires_self+=1
        if opp>0: fires_opp+=1
    shutil.rmtree(d,ignore_errors=True)
    print(f"  {name:10} {sum(gaps)/len(gaps):>+7.0f} {sum(1 for g in gaps if g>150):>8}/{len(gaps)}   eigene:{fires_self:2}  gegn:{fires_opp:2}")
