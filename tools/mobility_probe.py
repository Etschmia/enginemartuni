#!/usr/bin/env python3
"""Mobility-Probe Gate A: schrumpft der Optimismus-Gap auf den 35 Diagnose-
Stellungen, wenn die MG-Mobility-Gewichte angehoben werden?

Zerstoerungsfrei: erzeugt Test-eval.toml in Temp-Dirs, ruft martuni mit dieser
CWD (Kaskade: CWD zuerst). Live-eval.toml bleibt unberuehrt.
"""
import json, subprocess, re, os, tempfile, shutil
import chess

ROOT = "/home/librechat/enginemartuni"
MART = f"{ROOT}/target/release/martuni"
BASE_TOML = f"{ROOT}/eval.toml"

# Varianten: (name, {key: new_value}) — nur MG-Mobility, Dame fix
VARIANTS = {
    "baseline": {},
    "v1_minorsrook_x1.6": {"knight_mg": 5, "bishop_mg": 5, "rook_mg": 3},
    "v2_minorsrook_x2":   {"knight_mg": 6, "bishop_mg": 6, "rook_mg": 4},
}

r = json.load(open(f"{ROOT}/nomotif_verify.json"))
pos = [x for x in r if x["deep_loss"] >= 150 and abs(x["deep_after"]) < 5000]

def make_toml(changes):
    txt = open(BASE_TOML).read()
    for k, v in changes.items():
        txt = re.sub(rf"(?m)^{k}\s*=\s*-?\d+", f"{k} = {v}", txt)
    return txt

def mart_total(fen, cwd):
    out = subprocess.run([MART], input=f"position fen {fen}\neval\nquit\n",
                         capture_output=True, text=True, timeout=20, cwd=cwd).stdout
    m = re.search(r"info string total\s+(-?\d+)\s*cp", out)
    return int(m.group(1)) if m else None

# precompute after-fens + mover
items = []
for x in pos:
    b = chess.Board(x["fen"])
    mover = b.turn
    try:
        b.push(b.parse_san(x["mv"]))
    except Exception:
        continue
    items.append((b.fen(), mover, x["deep_after"], x["martuni_eval"]))

print(f"=== Mobility-Probe Gate A: {len(items)} positionelle After-Stellungen ===\n")
print(f"{'Variante':24} {'Ø mart_total':>12} {'Ø gap vs d26':>13} {'>150 rosy':>10}")
for name, changes in VARIANTS.items():
    d = tempfile.mkdtemp(prefix=f"mob_{name}_")
    open(os.path.join(d, "eval.toml"), "w").write(make_toml(changes))
    gaps = []; totals = []
    for fen, mover, deep_after, _ in items:
        t = mart_total(fen, d)
        if t is None:
            continue
        mt = t if mover == chess.WHITE else -t   # mover perspective
        totals.append(mt)
        gaps.append(mt - deep_after)
    shutil.rmtree(d, ignore_errors=True)
    avg_t = sum(totals)/len(totals)
    avg_g = sum(gaps)/len(gaps)
    rosy = sum(1 for g in gaps if g > 150)
    print(f"  {name:22} {avg_t:>+11.0f} {avg_g:>+12.0f} {rosy:>8}/{len(gaps)}")
print("\n  (d26-after Ø:", round(sum(d for _,_,d,_ in items)/len(items)), "cp — Ziel: gap -> 0)")
