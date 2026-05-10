#!/usr/bin/env python3
"""Erzeugt src/polyglot/martuni_patches.bin aus einer Liste manueller Eroeffnungs-Korrekturen.

Wird benoetigt, weil die GM-/Komodo-/Rodent-Standardbuecher bestimmte
Stellungen nicht abdecken, in denen Martuni in mehreren Partien denselben
Zug-Fehler wiederholt hat (z.B. ...Bxc5 statt ...Bc7 gegen sxphia).

Format: Polyglot-Standard. Die Polyglot-Hash-Implementierung von python-chess
ist identisch mit der in src/polyglot/hash.rs (gegengeprueft am Startpos-Hash
0x463b96181691fc9c).

Editieren -> ausfuehren -> neu builden:

    python3 tools/build_book_patches.py
    cargo build --release
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

import chess
import chess.polyglot

# -- Liste der Patches ------------------------------------------------------
# Jeder Eintrag: (FEN, UCI-Zug, weight, comment).
# Weight 1 reicht — nur die Move-Wahl zaehlt, nicht die Gewichtung.

PATCHES: list[tuple[str, str, int, str]] = [
    (
        # sxphia-Falle: Martuni schlaegt 4x in Folge ...Bxc5 (loss ~270cp,
        # bishop hangt nach dxc5). Korrekt waere Bc7.
        "r1bq1rk1/1p1n1ppp/p1pbpn2/2Pp4/3P2P1/2N1PN1P/PPQ2P2/R1B1KB1R b KQ - 0 9",
        "d6c7",
        1,
        "sxphia 9...Bxc5 hangs_bishop, replace with Bc7",
    ),
]

# Polyglot Move-Encoding (16 bit):
#   bits 0-2  to_file
#   bits 3-5  to_rank
#   bits 6-8  from_file
#   bits 9-11 from_rank
#   bits 12-14 promotion (0=none, 1=N, 2=B, 3=R, 4=Q)
PROMO_BITS = {None: 0, chess.KNIGHT: 1, chess.BISHOP: 2, chess.ROOK: 3, chess.QUEEN: 4}


def encode_move(board: chess.Board, mv: chess.Move) -> int:
    move = mv
    # Polyglot kodiert Rochade als "Koenig schlaegt eigenen Turm".
    if board.is_castling(mv):
        if chess.square_file(mv.to_square) == 6:  # kingside -> rook on h
            rook_file = 7
        else:
            rook_file = 0
        rook_rank = chess.square_rank(mv.from_square)
        target = chess.square(rook_file, rook_rank)
        move = chess.Move(mv.from_square, target)
    return (
        chess.square_file(move.to_square)
        | (chess.square_rank(move.to_square) << 3)
        | (chess.square_file(move.from_square) << 6)
        | (chess.square_rank(move.from_square) << 9)
        | (PROMO_BITS[move.promotion] << 12)
    )


def main() -> int:
    out_path = Path(__file__).resolve().parent.parent / "src" / "polyglot" / "martuni_patches.bin"

    rows: list[tuple[int, int, int, int]] = []  # key, move, weight, learn
    for fen, uci, weight, comment in PATCHES:
        board = chess.Board(fen)
        mv = chess.Move.from_uci(uci)
        if mv not in board.legal_moves:
            print(f"FEHLER: Zug {uci} ist in der Stellung nicht legal:\n  {fen}\n  ({comment})", file=sys.stderr)
            return 1
        key = chess.polyglot.zobrist_hash(board)
        encoded = encode_move(board, mv)
        rows.append((key, encoded, weight, 0))
        print(f"  +  key=0x{key:016x}  mv={uci} ({encoded:#06x})  w={weight}  // {comment}")

    # Polyglot-Files MUESSEN nach key sortiert sein (binary_search im Reader).
    rows.sort(key=lambda r: r[0])

    with out_path.open("wb") as fh:
        for key, mv, weight, learn in rows:
            fh.write(struct.pack(">QHHI", key, mv, weight, learn))

    print(f"\n{len(rows)} Eintrag/-Eintraege geschrieben -> {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
