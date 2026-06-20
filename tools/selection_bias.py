#!/usr/bin/env python3
"""Option 4: Selektions-Bias quantifizieren.

Misst denselben Gap (Martuni-%eval minus SF-d26-Eval der Stellung NACH dem Zug,
Mover-Sicht) auf einer ZUFALLS-Stichprobe von Martunis Mittelspielzuegen — und
filtert auf NICHT-Blunder (deep_loss < 150). Vergleich gegen den Cluster-Gap
(+250 cp median auf Blundern): ist der Optimismus cluster-spezifisch (= reine
Auswahl von Eval-Fehlmomenten) oder global (Martuni-Eval generell zu rosig /
~250 cp schwaecher als SF)? Inkrementelles Schreiben.
"""
import json, sys, glob, random
import chess, chess.pgn, chess.engine

SF="/usr/games/stockfish"; MT=2.0; OUT="selection_bias.json"; N=60
random.seed(42)
PV={chess.PAWN:100,chess.KNIGHT:320,chess.BISHOP:330,chess.ROOK:500,chess.QUEEN:900,chess.KING:0}

def npm(b):
    return sum(len(b.pieces(pt,c))*PV[pt] for pt in (chess.KNIGHT,chess.BISHOP,chess.ROOK,chess.QUEEN) for c in (True,False))
def phase(b):
    if b.fullmove_number<=12 and npm(b)>=5500: return "opening"
    if npm(b)<=2000: return "endgame"
    return "middlegame"
def cp(score,side):
    s=score.pov(side)
    if s.is_mate():
        m=s.mate(); return (100000-abs(m)*10) if m>0 else (-100000+abs(m)*10)
    return s.score()
def mart_eval(node):  # white-pov from sibling %eval
    if node.parent is None: return None
    for var in node.parent.variations:
        ev=var.eval()
        if ev is None: continue
        pov=ev.white()
        if pov.is_mate():
            m=pov.mate()
            return None if m is None else (100000-abs(m)*10 if m>0 else -100000+abs(m)*10)
        return pov.score()
    return None

# 1) Pool: Martuni-MG-Zuege mit %eval
pool=[]
for path in sorted(glob.glob("/home/librechat/lichess-bot/game_records/*.pgn")):
    with open(path,encoding="utf-8",errors="ignore") as fh:
        while True:
            try: g=chess.pgn.read_game(fh)
            except Exception: break
            if g is None: break
            w=g.headers.get("White",""); b=g.headers.get("Black","")
            ms = chess.WHITE if "Martuni" in w else chess.BLACK if "Martuni" in b else None
            if ms is None: continue
            board=g.board(); node=g
            while node.variations:
                nxt=node.variation(0); mv=nxt.move
                if board.turn==ms and phase(board)=="middlegame" and board.fullmove_number>=8:
                    mew=mart_eval(nxt)
                    if mew is not None:
                        pool.append({"fen":board.fen(),"san":board.san(mv),
                                     "mover":ms,"martuni_eval":mew if ms==chess.WHITE else -mew})
                board.push(mv); node=nxt
print(f"pool size: {len(pool)}",file=sys.stderr)
sample=random.sample(pool,min(N,len(pool)))

# 2) SF deep before+after
eng=chess.engine.SimpleEngine.popen_uci(SF); eng.configure({"Threads":4,"Hash":512})
lim=chess.engine.Limit(time=MT)
out=[]
for i,x in enumerate(sample):
    b=chess.Board(x["fen"]); mover=b.turn
    db=cp(eng.analyse(b,lim)["score"],mover)
    try: mv=b.parse_san(x["san"])
    except Exception: continue
    b.push(mv)
    da=cp(eng.analyse(b,lim)["score"],mover)
    out.append({"san":x["san"],"martuni_eval":x["martuni_eval"],"deep_before":db,
                "deep_after":da,"deep_loss":db-da,"gap":x["martuni_eval"]-da})
    json.dump(out,open(OUT,"w"),indent=1)
    if (i+1)%10==0: print(f"[{i+1}/{len(sample)}]",flush=True)
eng.quit()
print(f"Wrote {len(out)} to {OUT}",flush=True)
