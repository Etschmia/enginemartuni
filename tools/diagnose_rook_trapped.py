#!/usr/bin/env python3
"""Diagnose: in wie vielen Endgame-Blunder-Stellungen aus analyse_DD.MM.2026.txt
feuert die `rook_trapped_endgame_malus`-Logik aus src/eval.rs?

Nachbau der Rust-Logik (Stand 28.04.2026):
- Heimreihe + zweite Reihe der eigenen Seite (rank_index 0/1 für Weiß, 7/6 für Schwarz)
- eigener Bauer auf derselben Linie eine Reihe weiter vorne (Richtung Gegner)
- Pro Treffer: +1 Malus-Einheit

Ausgabe pro Eintrag und Aggregat — wir wollen wissen, ob der Term in den
Endgame-Blunder-Stellungen überhaupt anschlägt und wenn ja, ob das plausibel
aussieht oder Eval-Pessimismus erzeugt.
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

import chess


FEN_RE = re.compile(r"fen=(\S+(?: \S+){5})")
PHASE_RE = re.compile(r"phase=(\w+)")


def trapped_count(board: chess.Board, color: chess.Color) -> int:
    """1:1-Nachbau von rook_trapped_endgame_malus aus src/eval.rs."""
    if color == chess.WHITE:
        home_rank, second_rank, dir_ = 0, 1, 1
    else:
        home_rank, second_rank, dir_ = 7, 6, -1

    hits = 0
    for sq in board.pieces(chess.ROOK, color):
        rank = chess.square_rank(sq)
        if rank not in (home_rank, second_rank):
            continue
        blocker_rank = rank + dir_
        if not 0 <= blocker_rank <= 7:
            continue
        blocker_sq = chess.square(chess.square_file(sq), blocker_rank)
        piece = board.piece_at(blocker_sq)
        if piece and piece.piece_type == chess.PAWN and piece.color == color:
            hits += 1
    return hits


def main() -> int:
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <analyse_DD.MM.YYYY.txt>", file=sys.stderr)
        return 2

    path = Path(sys.argv[1])
    text = path.read_text(encoding="utf-8", errors="replace")

    total_endgame = 0
    fired_white = 0
    fired_black = 0
    fired_either = 0
    bilateral = 0
    fire_distribution: Counter[int] = Counter()
    samples: list[tuple[str, int, int]] = []

    for line in text.splitlines():
        if "phase=endgame" not in line:
            continue
        m_fen = FEN_RE.search(line)
        if not m_fen:
            continue
        fen = m_fen.group(1)
        try:
            board = chess.Board(fen)
        except ValueError:
            continue
        total_endgame += 1
        w = trapped_count(board, chess.WHITE)
        b = trapped_count(board, chess.BLACK)
        if w:
            fired_white += 1
        if b:
            fired_black += 1
        if w or b:
            fired_either += 1
            fire_distribution[(w, b)] += 1
            if len(samples) < 10:
                samples.append((fen, w, b))
        if w and b:
            bilateral += 1

    print(f"Endgame-Blunder mit FEN: {total_endgame}")
    print(f"  rook_trapped feuert für Weiß:        {fired_white}"
          f"  ({fired_white/max(total_endgame,1)*100:.1f} %)")
    print(f"  rook_trapped feuert für Schwarz:     {fired_black}"
          f"  ({fired_black/max(total_endgame,1)*100:.1f} %)")
    print(f"  feuert für mind. eine Seite:          {fired_either}"
          f"  ({fired_either/max(total_endgame,1)*100:.1f} %)")
    print(f"  feuert für beide Seiten gleichzeitig: {bilateral}")
    print()
    if fire_distribution:
        print("Verteilung (white_hits, black_hits) → Anzahl Stellungen:")
        for (w, b), n in sorted(fire_distribution.items(), key=lambda x: -x[1]):
            print(f"  ({w}, {b}): {n}")
        print()
        print("Stichproben (bis zu 10):")
        for fen, w, b in samples:
            print(f"  w={w} b={b}  {fen}")
    else:
        print("Term feuert in keiner einzigen Endgame-Blunder-Stellung.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
