#!/usr/bin/env python3
"""Groessere Stichprobe + Klassifikation der MG-motivlosen Drops.

Verifiziert ALLE middlegame-motivlosen Blunder bei SF (movetime) und zaehlt das
wiederkehrende AKTIONABLE Eval-Motiv aus, damit das naechste Feature den Cluster
auch trifft (Lehre aus dem Outpost-Fehlschuss). Inkrementelles Schreiben.

Themen (Prioritaet, sich ueberlappend mit ausgewiesen):
  - king_danger     : >=2 distinkte gegn. Angreifer auf Martunis Koenig-Zone
  - greedy_passive  : Martuni materiell >=+1, aber d26 <= -100 (Material > Stellung)
  - pawn_break      : SF-Bestzug ist ein Bauern-Vorstoss (Hebel)
  - other_passive   : Rest (reine Figuren-Passivitaet)
"""
import json, sys
import chess, chess.engine

SF="/usr/games/stockfish"; MT=0.8; OUT="cluster_classify.json"
d=json.load(open("analyse-04.06.2026.json"))
mg=[x for x in d["blunders"] if x["phase"]=="middlegame" and not x["motifs"]]

eng=chess.engine.SimpleEngine.popen_uci(SF); eng.configure({"Threads":4,"Hash":512})
lim=chess.engine.Limit(time=MT)
val={chess.PAWN:1,chess.KNIGHT:3,chess.BISHOP:3,chess.ROOK:5,chess.QUEEN:9}

def cp(score,pov):
    s=score.pov(pov)
    if s.is_mate(): m=s.mate(); return (100000-abs(m)*10) if m>0 else (-100000+abs(m)*10)
    return s.score()

def king_danger(board, defender):
    """# distinkte gegn. Angreifer auf die 3x3-Zone um defenders Koenig."""
    ksq=board.king(defender)
    if ksq is None: return 0
    zone=set([ksq]) | set(chess.SquareSet(chess.BB_KING_ATTACKS[ksq]))
    attackers=set()
    for sq in zone:
        attackers |= set(board.attackers(not defender, sq))
    return len(attackers)

def material(board, mover):
    return sum(v*(len(board.pieces(pt,mover))-len(board.pieces(pt,not mover))) for pt,v in val.items())

out=[]
for i,x in enumerate(mg):
    b=chess.Board(x["fen_before"]); mover=b.turn
    info=eng.analyse(b,lim)
    deep_before=cp(info["score"],mover)
    pv=info.get("pv",[]); best=pv[0] if pv else None
    best_pt = b.piece_at(best.from_square).piece_type if best else None
    best_cap = b.is_capture(best) if best else False
    best_kind = ("capture" if best_cap else
                 "pawn_push" if best_pt==chess.PAWN else
                 "king" if best_pt==chess.KING else "piece")
    try: mv=b.parse_san(x["move_san"])
    except Exception: continue
    b.push(mv)
    deep_after=cp(eng.analyse(b,lim)["score"],mover)
    kd=king_danger(b,mover)           # Martunis Koenig nach seinem Zug
    mat=material(b,mover)
    out.append({"mv":x["move_san"],"deep_before":deep_before,"deep_after":deep_after,
                "deep_loss":deep_before-deep_after,"king_danger":kd,"material":mat,
                "best_kind":best_kind,"martuni_eval":x["martuni_eval_cp"]})
    json.dump(out,open(OUT,"w"),indent=1)
    if (i+1)%20==0: print(f"[{i+1}/{len(mg)}]",flush=True)
eng.quit()
print(f"Wrote {len(out)} to {OUT}",flush=True)
