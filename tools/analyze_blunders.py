#!/usr/bin/env python3
"""Analyze PGN games with Stockfish, detect blunders, group by phase and motif."""

from __future__ import annotations

import argparse
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable

import chess
import chess.engine
import chess.pgn


PIECE_VALUES = {
    chess.PAWN: 100,
    chess.KNIGHT: 320,
    chess.BISHOP: 330,
    chess.ROOK: 500,
    chess.QUEEN: 900,
    chess.KING: 0,
}


@dataclass
class Blunder:
    game_id: str
    ply: int
    move_number: int
    side: str
    move_san: str
    fen_before: str
    eval_before_cp: int
    eval_after_cp: int
    loss_cp: int
    phase: str
    motifs: list[str]
    best_move_san: str | None


@dataclass
class Report:
    blunders: list[Blunder] = field(default_factory=list)

    def by_phase(self) -> dict[str, list[Blunder]]:
        out: dict[str, list[Blunder]] = defaultdict(list)
        for b in self.blunders:
            out[b.phase].append(b)
        return out

    def by_motif(self) -> dict[str, list[Blunder]]:
        out: dict[str, list[Blunder]] = defaultdict(list)
        for b in self.blunders:
            for m in b.motifs or ["unclassified"]:
                out[m].append(b)
        return out

    def by_phase_and_motif(self) -> dict[tuple[str, str], list[Blunder]]:
        out: dict[tuple[str, str], list[Blunder]] = defaultdict(list)
        for b in self.blunders:
            for m in b.motifs or ["unclassified"]:
                out[(b.phase, m)].append(b)
        return out


def non_pawn_material(board: chess.Board) -> int:
    total = 0
    for piece_type in (chess.KNIGHT, chess.BISHOP, chess.ROOK, chess.QUEEN):
        total += len(board.pieces(piece_type, chess.WHITE)) * PIECE_VALUES[piece_type]
        total += len(board.pieces(piece_type, chess.BLACK)) * PIECE_VALUES[piece_type]
    return total


def detect_phase(board: chess.Board) -> str:
    npm = non_pawn_material(board)
    full_move = board.fullmove_number
    if full_move <= 12 and npm >= 5500:
        return "opening"
    if npm <= 2000:
        return "endgame"
    return "middlegame"


def score_to_cp(score: chess.engine.PovScore, side: chess.Color) -> int:
    """Return centipawn score from the given side's perspective.

    Mate scores are clamped to a large value so that comparisons still work.
    """
    pov = score.pov(side)
    if pov.is_mate():
        mate = pov.mate()
        if mate is None:
            return 0
        # Closer mates should be more extreme.
        return 100000 - abs(mate) * 10 if mate > 0 else -100000 + abs(mate) * 10
    return pov.score(mate_score=100000)


def is_hanging(board: chess.Board, square: chess.Square) -> bool:
    """Heuristic: piece on `square` is attacked by side-to-move and undefended or under-defended."""
    piece = board.piece_at(square)
    if piece is None:
        return False
    attackers = board.attackers(not piece.color, square)
    if not attackers:
        return False
    defenders = board.attackers(piece.color, square)
    if not defenders:
        return True
    # Very rough SEE-lite: compare lowest attacker vs lowest defender value.
    min_attacker = min(PIECE_VALUES[board.piece_at(s).piece_type] for s in attackers)
    piece_val = PIECE_VALUES[piece.piece_type]
    return min_attacker < piece_val


def classify_motifs(
    board_before: chess.Board,
    board_after: chess.Board,
    move: chess.Move,
    best_move: chess.Move | None,
    eval_before: int,
    eval_after: int,
    info_before: chess.engine.InfoDict,
    info_after: chess.engine.InfoDict,
) -> list[str]:
    motifs: list[str] = []
    mover = board_before.turn

    # Missed mate: engine announced mate for mover, user played something else.
    pov_before = info_before.get("score")
    if pov_before is not None:
        pov = pov_before.pov(mover)
        if pov.is_mate() and pov.mate() is not None and pov.mate() > 0:
            if best_move is not None and move != best_move:
                motifs.append("missed_mate")

    # Walked into mate: after the move, opponent has mate.
    pov_after = info_after.get("score")
    if pov_after is not None:
        pov = pov_after.pov(not mover)
        if pov.is_mate() and pov.mate() is not None and pov.mate() > 0:
            motifs.append("allows_mate")

    # Hanging own piece: some own piece of ours is en prise after the move.
    for square in chess.SQUARES:
        piece = board_after.piece_at(square)
        if piece and piece.color == mover and piece.piece_type != chess.PAWN:
            if is_hanging(board_after, square):
                motifs.append(f"hangs_{chess.piece_name(piece.piece_type)}")
                break

    # Missed capture: best move was a capture we didn't make.
    if best_move is not None and move != best_move:
        if board_before.is_capture(best_move):
            motifs.append("missed_capture")

    # King safety degradation: opponent attackers near our king increased sharply.
    king_sq = board_after.king(mover)
    if king_sq is not None:
        zone = chess.SquareSet(chess.BB_KING_ATTACKS[king_sq]) | chess.SquareSet(chess.BB_SQUARES[king_sq])
        attackers = sum(1 for sq in zone if board_after.attackers(not mover, sq))
        if attackers >= 4:
            motifs.append("king_safety")

    # Material swing without compensation: eval loss very large and no motif fired yet.
    if not motifs and (eval_before - eval_after) >= 300:
        motifs.append("positional_collapse")

    return motifs


def analyze_game(
    engine: chess.engine.SimpleEngine,
    game: chess.pgn.Game,
    limit: chess.engine.Limit,
    threshold_cp: int,
    game_id: str,
) -> Iterable[Blunder]:
    board = game.board()
    ply = 0
    for move in game.mainline_moves():
        ply += 1
        mover = board.turn
        info_before = engine.analyse(board, limit)
        eval_before = score_to_cp(info_before["score"], mover)
        best_move = info_before.get("pv", [None])[0]

        board_after = board.copy(stack=False)
        board_after.push(move)

        info_after = engine.analyse(board_after, limit)
        eval_after_from_mover = score_to_cp(info_after["score"], mover)
        loss = eval_before - eval_after_from_mover

        if loss >= threshold_cp:
            phase = detect_phase(board)
            motifs = classify_motifs(
                board,
                board_after,
                move,
                best_move,
                eval_before,
                eval_after_from_mover,
                info_before,
                info_after,
            )
            yield Blunder(
                game_id=game_id,
                ply=ply,
                move_number=board.fullmove_number,
                side="white" if mover == chess.WHITE else "black",
                move_san=board.san(move),
                fen_before=board.fen(),
                eval_before_cp=eval_before,
                eval_after_cp=eval_after_from_mover,
                loss_cp=loss,
                phase=phase,
                motifs=motifs,
                best_move_san=board.san(best_move) if best_move else None,
            )

        board.push(move)


def iter_games(pgn_paths: list[Path]) -> Iterable[tuple[str, chess.pgn.Game]]:
    for path in pgn_paths:
        with path.open("r", encoding="utf-8") as fh:
            idx = 0
            while True:
                game = chess.pgn.read_game(fh)
                if game is None:
                    break
                idx += 1
                white = game.headers.get("White", "?")
                black = game.headers.get("Black", "?")
                yield f"{path.name}#{idx} {white}-{black}", game


def print_report(report: Report) -> None:
    if not report.blunders:
        print("No blunders above threshold.")
        return

    total = len(report.blunders)
    print(f"\n=== Summary ({total} blunders) ===\n")

    print("By phase:")
    for phase, blunders in sorted(report.by_phase().items()):
        print(f"  {phase:<12} {len(blunders):>4}")
    print()

    print("By motif:")
    for motif, blunders in sorted(report.by_motif().items(), key=lambda kv: -len(kv[1])):
        print(f"  {motif:<24} {len(blunders):>4}")
    print()

    print("By phase x motif:")
    for (phase, motif), blunders in sorted(
        report.by_phase_and_motif().items(), key=lambda kv: (kv[0][0], -len(kv[1]))
    ):
        print(f"  {phase:<12} {motif:<24} {len(blunders):>4}")
    print()

    print("=== Details ===")
    for b in report.blunders:
        best = f" best={b.best_move_san}" if b.best_move_san else ""
        motifs = ",".join(b.motifs) if b.motifs else "unclassified"
        print(
            f"[{b.game_id}] {b.move_number}{'.' if b.side == 'white' else '...'} "
            f"{b.move_san}  loss={b.loss_cp}cp  phase={b.phase}  motifs={motifs}{best}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pgn", nargs="+", type=Path, help="PGN file(s) to analyze")
    parser.add_argument(
        "--engine",
        default="stockfish",
        help="Path to Stockfish binary (default: 'stockfish' on PATH)",
    )
    parser.add_argument(
        "--movetime",
        type=float,
        default=0.3,
        help="Seconds per analysis (default: 0.3)",
    )
    parser.add_argument(
        "--depth",
        type=int,
        default=None,
        help="Use fixed search depth instead of movetime",
    )
    parser.add_argument(
        "--threshold",
        type=int,
        default=150,
        help="Centipawn loss that qualifies as a blunder (default: 150)",
    )
    parser.add_argument(
        "--hash",
        type=int,
        default=128,
        help="Stockfish hash size in MB",
    )
    parser.add_argument(
        "--threads",
        type=int,
        default=1,
        help="Stockfish threads",
    )
    args = parser.parse_args()

    for p in args.pgn:
        if not p.exists():
            print(f"error: {p} not found", file=sys.stderr)
            return 2

    limit = (
        chess.engine.Limit(depth=args.depth)
        if args.depth is not None
        else chess.engine.Limit(time=args.movetime)
    )

    try:
        engine = chess.engine.SimpleEngine.popen_uci(args.engine)
    except FileNotFoundError:
        print(f"error: engine '{args.engine}' not found", file=sys.stderr)
        return 2

    try:
        engine.configure({"Hash": args.hash, "Threads": args.threads})
    except chess.engine.EngineError:
        pass

    report = Report()
    try:
        for game_id, game in iter_games(args.pgn):
            for blunder in analyze_game(engine, game, limit, args.threshold, game_id):
                report.blunders.append(blunder)
    finally:
        engine.quit()

    print_report(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
