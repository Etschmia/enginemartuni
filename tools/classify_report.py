#!/usr/bin/env python3
"""Auswertung cluster_classify.json: welches AKTIONABLE Eval-Motiv dominiert?"""
import json, statistics
from collections import Counter
r=json.load(open("cluster_classify.json"))
n=len(r)
surv=[x for x in r if x["deep_loss"]>=150]
mate=[x for x in surv if abs(x["deep_after"])>=5000]
pos=[x for x in surv if abs(x["deep_after"])<5000]
print(f"=== {n} MG-motivlose Stellungen bei SF (movetime) ===")
print(f"  ueberleben (deep_loss>=150): {len(surv)} ({len(surv)/n:.0%})")
print(f"    davon Tiefen-Matt/Taktik:  {len(mate)} (Such-Seite)")
print(f"    davon echt positionell:    {len(pos)} (Eval-Kandidat)\n")

def gap(x): return (x["martuni_eval"]-x["deep_after"]) if x["martuni_eval"] is not None else None

# Themen-Klassifikation (Prioritaet) ueber die positionellen Survivors
def theme(x):
    if x["king_danger"]>=2: return "king_danger"
    if x["material"]>=1 and x["deep_after"]<=-100: return "greedy_passive"
    if x["best_kind"]=="pawn_push": return "pawn_break"
    return "other_passive"

th=Counter(theme(x) for x in pos)
print("=== Dominantes Motiv (Prioritaet, positionelle Survivors) ===")
for k,c in th.most_common():
    sub=[x for x in pos if theme(x)==k]
    gaps=[gap(x) for x in sub if gap(x) is not None]
    g=round(statistics.median(gaps)) if gaps else None
    print(f"  {k:16} {c:3} ({c/len(pos):.0%})   median gap {g}")

print("\n=== Roh-Verteilungen (ueberlappend) ===")
print(f"  king_danger>=2 Angreifer: {sum(1 for x in pos if x['king_danger']>=2)} / {len(pos)} ({sum(1 for x in pos if x['king_danger']>=2)/len(pos):.0%})")
print(f"  king_danger>=3 Angreifer: {sum(1 for x in pos if x['king_danger']>=3)} / {len(pos)}")
print(f"  Martuni materiell >=+1:   {sum(1 for x in pos if x['material']>=1)} / {len(pos)} ({sum(1 for x in pos if x['material']>=1)/len(pos):.0%})")
print(f"  Martuni mat>=+1 & losing: {sum(1 for x in pos if x['material']>=1 and x['deep_after']<=-100)} / {len(pos)}")
print(f"  SF-Bestzug Art:           {dict(Counter(x['best_kind'] for x in pos))}")
print(f"  median material (mover):  {statistics.median([x['material'] for x in pos])}")
gaps=[gap(x) for x in pos if gap(x) is not None]
print(f"  median Optimismus-Gap:    {round(statistics.median(gaps))} cp  (>150: {sum(1 for g in gaps if g>150)}/{len(gaps)})")
