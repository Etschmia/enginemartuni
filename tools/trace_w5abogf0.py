#!/usr/bin/env python3
"""Deep-Dive auf W5AboGf0 m29 W — warum spielt Martuni Qg4 statt Qxd6?

Martuni unterstuetzt `searchmoves` nicht. Wir umgehen das, indem wir den
Kandidaten-Zug auf dem Brett ausfuehren (python-chess) und die Engine die
Folge-Stellung bewerten lassen. Ergebnis: das ist die Bewertung aus Schwarz'
Sicht (`-` invertieren fuer Weiss).

Tests:
  (1) Free-search an der Wurzel: was waehlt die Engine, in welcher Tiefe?
  (2) Pro Kandidat: Post-Move-Eval (Black to move), invertiert.
  (3) SEE-Werte fuer die Captures (ueber python-chess approximiert).
"""

from __future__ import annotations

import chess
import subprocess
from pathlib import Path

ENGINE = Path(__file__).resolve().parent.parent / "target" / "release" / "martuni"
FEN = "5r1k/5nbp/3qQpp1/4p3/1r2P3/p5B1/P2N1PPP/R2R2K1 w - - 2 29"

CANDIDATES = [
    ("e6d6", "Qxd6  — Stockfish-best, schluckt Schwarz' Dame"),
    ("e6g4", "Qg4   — Engine-Wahl in der echten Partie"),
    ("e6e5", "Qxe5  — Bauern-Capture"),
    ("g3e5", "Bxe5  — Bauern-Capture mit Laeufer"),
    ("g3f4", "Bf4   — quiet move, vergleichswert"),
]


def uci_search(fen: str, depth: int = 12, movetime_ms: int = 18_000) -> tuple[str | None, list[tuple[int, int | None, str | None]]]:
    proc = subprocess.Popen(
        [str(ENGINE)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, bufsize=1,
    )
    assert proc.stdin and proc.stdout

    def send(line: str) -> None:
        proc.stdin.write(line + "\n"); proc.stdin.flush()

    send("uci")
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "uciok":
            break
    send("setoption name Hash value 256")
    send("isready")
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "readyok":
            break
    send("ucinewgame")
    send(f"position fen {fen}")
    send(f"go depth {depth} movetime {movetime_ms}")

    rows: list[tuple[int, int | None, str | None]] = []
    best: str | None = None
    while True:
        line = proc.stdout.readline()
        if not line:
            break
        line = line.rstrip()
        if line.startswith("info "):
            toks = line.split()
            try:
                d = int(toks[toks.index("depth") + 1])
            except (ValueError, IndexError):
                continue
            sc = None
            if "score" in toks:
                s = toks.index("score")
                if toks[s+1] == "cp":
                    try: sc = int(toks[s+2])
                    except: pass
                elif toks[s+1] == "mate":
                    try:
                        v = int(toks[s+2])
                        sc = (99000 - abs(v)) * (1 if v > 0 else -1)
                    except: pass
            pv = None
            if "pv" in toks:
                i = toks.index("pv")
                if i + 1 < len(toks): pv = toks[i+1]
            rows.append((d, sc, pv))
        elif line.startswith("bestmove"):
            parts = line.split()
            if len(parts) >= 2: best = parts[1]
            break

    send("quit")
    try: proc.wait(timeout=2)
    except subprocess.TimeoutExpired: proc.kill()
    return best, rows


def fmt(s: int | None) -> str:
    if s is None: return "?    "
    if abs(s) >= 90000: return f"M{99000-abs(s):+d}"
    return format(s, "+5d")


def main() -> int:
    print(f"# Trace W5AboGf0 m29 W   FEN: {FEN}\n")

    # ---- (1) Free-search ---------------------------------------------------
    print("## (1) Free-Search Wurzel (Engine darf alles)")
    best, rows = uci_search(FEN, depth=14, movetime_ms=25_000)
    print(f"  final bestmove: {best}")
    print("  PV-Erstzug & Score pro Tiefe:")
    for d,s,m in rows:
        print(f"     d{d:>2}  {fmt(s)} cp   pv0={m}")
    print()

    # ---- (3) SEE statisch (python-chess approx, nur fuer Captures) --------
    # SEE-Hilfe: wir simulieren naiv (alle Capture-Reihenfolgen werden NICHT
    # exhaustiv durchgespielt; das ist nur ein grobes Material-Diff)
    print("## (3) Materialbilanz der Captures (NUR Capture/Recapture-Kette)")
    board = chess.Board(FEN)
    piece_vals = {chess.PAWN: 100, chess.KNIGHT: 320, chess.BISHOP: 330,
                  chess.ROOK: 500, chess.QUEEN: 900, chess.KING: 20000}
    for uci, desc in CANDIDATES:
        mv = chess.Move.from_uci(uci)
        if not board.is_capture(mv):
            print(f"  {uci}  (kein Schlag)")
            continue
        # SEE-naiv: GAIN = Wert des geschlagenen Stuecks - Wert der schlagenden Figur, FALLS Gegner zurueckschlaegt
        captured_pt = chess.PAWN if board.is_en_passant(mv) else board.piece_type_at(mv.to_square)
        captured_val = piece_vals.get(captured_pt, 0)
        attacker_val = piece_vals[board.piece_type_at(mv.from_square)]
        # Wer kann zurueckschlagen?
        post = board.copy(); post.push(mv)
        recaptures = [m for m in post.legal_moves if m.to_square == mv.to_square]
        if not recaptures:
            net = captured_val
            print(f"  {uci}  Schlag={captured_val:>4}  kein Recapture  NET={net:+5d}   ({desc})")
        else:
            # billigste Rueckschlag-Figur
            cheapest = min(piece_vals[post.piece_type_at(m.from_square)] for m in recaptures)
            net = captured_val - attacker_val
            # falls weitere Recapture-Kette: vereinfacht weglassen, NET ist erste Differenz
            print(f"  {uci}  Schlag={captured_val:>4}  vs Attacker={attacker_val:>4}, billigster Re-Schlaeger={cheapest:>4}  NET≈{net:+5d}   ({desc})")
    print()

    # ---- (2) Post-Move-Eval (Black to move) -------------------------------
    print("## (2) Post-Move-Eval: nach jedem Kandidat ist Schwarz am Zug, Engine bewertet.")
    print("       White-Eval = -BlackEval. Tiefe 14 max, 18 s Budget.")
    print()
    results = []
    for uci, desc in CANDIDATES:
        b = chess.Board(FEN)
        mv = chess.Move.from_uci(uci)
        if mv not in b.legal_moves:
            print(f"  {uci}  ILLEGAL"); continue
        b.push(mv)
        post_fen = b.fen()
        _, rows = uci_search(post_fen, depth=14, movetime_ms=18_000)
        with_score = [(d,s,m) for d,s,m in rows if s is not None]
        if not with_score:
            print(f"  {uci} = {desc}:  (keine Score-Zeilen)"); continue
        last_d, last_black_s, last_pv = with_score[-1]
        white_eval = -last_black_s if abs(last_black_s) < 90000 else -last_black_s
        results.append((uci, desc, last_d, white_eval, last_pv))
        # tail
        tail = with_score[-5:]
        tail_str = "  ".join(f"d{d:>2}:{fmt(-s)}" for d,s,_ in tail)
        print(f"  {uci} = {desc}")
        print(f"     White-Eval bei d{last_d}: {fmt(white_eval)} cp   (Black PV-Antwort: {last_pv})")
        print(f"     Verlauf (White-Sicht): {tail_str}")
    print()

    print("## (4) Ranking nach White-Eval (hoeher = besser fuer Weiss)")
    ranked = sorted([r for r in results if r[3] is not None], key=lambda r: -r[3])
    for i, (uci, desc, d, s, _pv) in enumerate(ranked, 1):
        s_str = fmt(s)
        print(f"  Platz {i}: {uci:6s}  {s_str} cp   {desc}")
    print()
    if best and best != "e6d6":
        print(f"  → Engine spielt {best}. Falls {ranked[0][0]} oben in der Post-Move-Eval steht aber nicht gewaehlt wird,")
        print(f"    ist das ein Move-Ordering- oder Search-Pruning-Effekt, kein statischer Eval-Bug.")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
