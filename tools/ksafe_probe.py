#!/usr/bin/env python3
"""King-Safety-Probe Gate A: schrumpft der Optimismus-Gap, wenn Angreifer-
Gewichte / SafetyTable angehoben werden? Zerstoerungsfrei (Temp-CWD)."""
import json, subprocess, re, os, tempfile, shutil
import chess

ROOT = "/home/librechat/enginemartuni"
MART = f"{ROOT}/target/release/martuni"
BASE = open(f"{ROOT}/eval.toml").read()

def scale_table(txt, factor, cap=700):
    m = re.search(r"safety_table = \[(.*?)\]", txt, re.S)
    nums = [int(n) for n in re.findall(r"-?\d+", m.group(1))]
    scaled = [min(round(n*factor), cap) for n in nums]
    body = ",".join(str(n) for n in scaled)
    return txt[:m.start()] + f"safety_table = [{body}]" + txt[m.end():]

def variant(weights=None, table_factor=None):
    txt = BASE
    for k, v in (weights or {}).items():
        txt = re.sub(rf"(?m)^{k}\s*=\s*-?\d+", f"{k} = {v}", txt)
    if table_factor:
        txt = scale_table(txt, table_factor)
    return txt

VARIANTS = {
    "baseline": variant(),
    "kw_x1.6": variant({"knight_weight":3,"bishop_weight":3,"rook_weight":5,"queen_weight":8}),
    "kw_x2+tbl1.5": variant({"knight_weight":4,"bishop_weight":4,"rook_weight":6,"queen_weight":10}, table_factor=1.5),
}

r = json.load(open(f"{ROOT}/nomotif_verify.json"))
pos = [x for x in r if x["deep_loss"] >= 150 and abs(x["deep_after"]) < 5000]

def mart_total(fen, cwd):
    out = subprocess.run([MART], input=f"position fen {fen}\neval\nquit\n",
                         capture_output=True, text=True, timeout=20, cwd=cwd).stdout
    m = re.search(r"info string total\s+(-?\d+)\s*cp", out)
    ks = re.search(r"king_safety\s+W=\s*(-?\d+)\s+B=\s*(-?\d+)", out)
    return (int(m.group(1)) if m else None,
            (int(ks.group(1)), int(ks.group(2))) if ks else None)

items = []
for x in pos:
    b = chess.Board(x["fen"]); mover = b.turn
    try: b.push(b.parse_san(x["mv"]))
    except Exception: continue
    items.append((b.fen(), mover, x["deep_after"]))

print(f"=== King-Safety-Probe Gate A: {len(items)} Stellungen ===\n")
print(f"{'Variante':16} {'Ø mart_total':>12} {'Ø gap vs d26':>13} {'>150 rosy':>10}")
for name, txt in VARIANTS.items():
    d = tempfile.mkdtemp(prefix=f"ks_")
    open(os.path.join(d,"eval.toml"),"w").write(txt)
    gaps=[]; totals=[]
    for fen, mover, da in items:
        t,_ = mart_total(fen, d)
        if t is None: continue
        mt = t if mover==chess.WHITE else -t
        totals.append(mt); gaps.append(mt-da)
    shutil.rmtree(d, ignore_errors=True)
    print(f"  {name:14} {sum(totals)/len(totals):>+11.0f} {sum(gaps)/len(gaps):>+12.0f} {sum(1 for g in gaps if g>150):>8}/{len(gaps)}")
print("\n  (d26-after Ø:", round(sum(d for _,_,d in items)/len(items)), "cp — Ziel: gap -> 0)")
