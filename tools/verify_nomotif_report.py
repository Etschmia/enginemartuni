#!/usr/bin/env python3
"""Auswertung von nomotif_verify.json: ueberlebt der Verlust die Tiefe?"""
import json, statistics
r = json.load(open("nomotif_verify.json"))
n = len(r)
print(f"=== Tief-Verifikation: {n} motivlose Blunder bei SF d26 ===\n")

# 1) Survival: wie viele "Verluste" ueberleben?
surv150 = [x for x in r if x["deep_loss"] >= 150]
surv100 = [x for x in r if x["deep_loss"] >= 100]
collapse = [x for x in r if x["deep_loss"] < 50]
neg = [x for x in r if x["deep_loss"] <= 0]
print("Survival des Verlusts bei d26:")
print(f"  deep_loss >= 150 (echter Blunder):  {len(surv150):3} ({len(surv150)/n:.0%})")
print(f"  deep_loss >= 100:                   {len(surv100):3} ({len(surv100)/n:.0%})")
print(f"  deep_loss <  50 (verpufft/Noise):   {len(collapse):3} ({len(collapse)/n:.0%})")
print(f"  deep_loss <= 0 (Zug war OK/besser): {len(neg):3} ({len(neg)/n:.0%})")
sl = [x["shallow_loss"] for x in r]; dl = [x["deep_loss"] for x in r]
print(f"\n  shallow_loss median {round(statistics.median(sl))}  vs  deep_loss median {round(statistics.median(dl))}")
print(f"  mean shallow {round(statistics.mean(sl))} vs deep {round(statistics.mean(dl))}")

# 2) Fuer die Ueberlebenden: Eval-Blindheit vs Search/forced
print("\n=== Survivors (deep_loss>=150): Eval-Blindheit? ===")
blind = []; saw = []
for x in surv150:
    me = x["martuni_eval"]
    if me is None:
        continue
    gap = me - x["deep_after"]   # mover pov, >0 = Martuni zu optimistisch ggue. Tiefe
    (blind if gap > 150 else saw).append((gap, x))
print(f"  mit martuni_eval: {len(blind)+len(saw)}")
print(f"  EVAL-BLIND (martuni_eval >150cp optimistischer als d26-after): {len(blind)}")
print(f"  SAH-ES-KOMMEN (martuni_eval ~ d26-after, <150 gap):            {len(saw)}")

# 3) Phase-Split der Survivors
from collections import Counter
print("\n  Survivor-Phasen:", dict(Counter(x["phase"] for x in surv150)))

# 4) Liste der haertesten echten Survivors
print("\n=== Haerteste echte Survivors (deep_loss, mit Eval-Gap) ===")
def gap(x): return (x["martuni_eval"]-x["deep_after"]) if x["martuni_eval"] is not None else None
for x in sorted(surv150, key=lambda z:-z["deep_loss"])[:15]:
    g = gap(x)
    print(f"  {x['phase'][:3]} {x['mv']:6} deep_loss{x['deep_loss']:5} (d26 {x['deep_before']:5}->{x['deep_after']:5}) mEval {str(x['martuni_eval']):>6} gap {str(g):>6} sf_best {str(x['sf_best']):8}")
    print(f"      {x['fen']}")
