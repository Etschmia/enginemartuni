#!/usr/bin/env python3
"""Qualitative Diagnose: fuer die klarsten positionellen Survivors den SF-Plan
(PV nach Bestzug) vs. Martunis Zug zeigen + Materialbilanz."""
import json, chess, chess.engine

SF="/usr/games/stockfish"
r=json.load(open("/home/librechat/enginemartuni/nomotif_verify.json"))
pos=[x for x in r if x["deep_loss"]>=150 and abs(x["deep_after"])<5000 and x["martuni_eval"] is not None]
# clearest: biggest gap
for x in pos:
    x["gap"]=x["martuni_eval"]-x["deep_after"]
pos.sort(key=lambda z:-z["gap"])
sel=pos[:6]

val={chess.PAWN:1,chess.KNIGHT:3,chess.BISHOP:3,chess.ROOK:5,chess.QUEEN:9}
def matbal(board, mover):
    s=0
    for pt,v in val.items():
        s+=v*(len(board.pieces(pt,mover))-len(board.pieces(pt,not mover)))
    return s

eng=chess.engine.SimpleEngine.popen_uci(SF); eng.configure({"Threads":4,"Hash":512})
lim=chess.engine.Limit(time=2.0)
for x in sel:
    b=chess.Board(x["fen"]); mover=b.turn
    mb=matbal(b,mover)
    # SF plan from before-position
    info=eng.analyse(b,lim,multipv=1)
    pv=info[0].get("pv",[]) if isinstance(info,list) else info.get("pv",[])
    plan=b.variation_san(pv[:7]) if pv else "?"
    print(f"\n{'='*70}")
    print(f"{x['game']}")
    print(f"FEN: {x['fen']}")
    side='Weiss' if mover==chess.WHITE else 'Schwarz'
    print(f"Martuni ({side}, Material {mb:+d}) spielte: {x['mv']}   (martuni %eval {x['martuni_eval']:+d})")
    print(f"  -> nach Zug d26 = {x['deep_after']:+d}  => Optimismus-Gap {x['gap']:+d} cp")
    print(f"SF-Bestzug+Plan: {plan}")
eng.quit()
