#!/usr/bin/env python3
"""Analyze PGN games with Stockfish, detect blunders, group by phase and motif."""

from __future__ import annotations

import argparse
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timezone
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
    # Martuni's own eval at this position (from PGN variation node), None if unavailable
    martuni_eval_cp: int | None


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


def player_colors(game: chess.pgn.Game, player: str) -> set[chess.Color]:
    """Return the set of colors the target player controlled in this game.

    Matching is case-insensitive substring on White/Black headers so that
    "Martuni" matches "Martuni 0.3", "martuni-dev", etc.
    """
    needle = player.lower()
    colors: set[chess.Color] = set()
    if needle in game.headers.get("White", "").lower():
        colors.add(chess.WHITE)
    if needle in game.headers.get("Black", "").lower():
        colors.add(chess.BLACK)
    return colors


def martuni_lost(game: chess.pgn.Game, player: str, target_colors: set[chess.Color]) -> bool:
    """Return True if the target player lost this game."""
    result = game.headers.get("Result", "*")
    if chess.WHITE in target_colors and result == "0-1":
        return True
    if chess.BLACK in target_colors and result == "1-0":
        return True
    return False


def read_martuni_eval(node: chess.pgn.GameNode) -> int | None:
    """Read Martuni's own eval from the PGN (lichess-bot format).

    lichess-bot schreibt den Eval als Geschwisterknoten des Hauptzugs:
      10. Qf4 { [%clk ...] } ( 10. Qf4 { [%eval -0.91,3] } )
    Der %eval hängt also an einer anderen Variation desselben Elternknotens,
    nicht an einem Kind des aktuellen Knotens.
    Returns centipawns from White's perspective, or None.
    """
    if node.parent is None:
        return None
    for var in node.parent.variations:
        ev = var.eval()
        if ev is None:
            continue
        pov = ev.white()
        if pov.is_mate():
            mate = pov.mate()
            if mate is None:
                return None
            return 100000 - abs(mate) * 10 if mate > 0 else -100000 + abs(mate) * 10
        score = pov.score()
        return score if score is not None else None
    return None


def analyze_game(
    engine: chess.engine.SimpleEngine,
    game: chess.pgn.Game,
    limit: chess.engine.Limit,
    threshold_cp: int,
    game_id: str,
    target_colors: set[chess.Color],
    min_move_time: float = 0.0,
) -> Iterable[Blunder]:
    board = game.board()
    ply = 0
    node = game
    # Track last seen clock per color to compute time spent per move.
    last_clock: dict[chess.Color, float] = {}
    skipped_fast = 0

    for move in game.mainline_moves():
        ply += 1
        mover = board.turn
        node = node.next()  # type: ignore[assignment]

        # Compute time spent on this move from %clk annotations.
        curr_clock = node.clock() if node is not None else None
        time_spent: float | None = None
        if curr_clock is not None and mover in last_clock:
            time_spent = last_clock[mover] - curr_clock
        if curr_clock is not None:
            last_clock[mover] = curr_clock

        if mover not in target_colors:
            board.push(move)
            continue

        # Skip moves played almost instantly (book moves, pre-moves).
        if min_move_time > 0.0 and time_spent is not None and time_spent < min_move_time:
            skipped_fast += 1
            board.push(move)
            continue

        # Read Martuni's own eval for this move (stored in PGN variation)
        martuni_eval_white: int | None = None
        if node is not None:
            martuni_eval_white = read_martuni_eval(node)

        info_before = engine.analyse(board, limit)
        eval_before = score_to_cp(info_before["score"], mover)
        best_move = info_before.get("pv", [None])[0]

        board_after = board.copy(stack=False)
        board_after.push(move)

        info_after = engine.analyse(board_after, limit)
        eval_after_from_mover = score_to_cp(info_after["score"], mover)
        loss = eval_before - eval_after_from_mover

        if loss >= threshold_cp:
            # Kein echter Blunder: Martuni spielte exakt den SF-empfohlenen Zug.
            # Die Stellung war bereits verloren — das ist ein false positive im Analyzer.
            if best_move is not None and move == best_move:
                board.push(move)
                continue

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
            # Convert Martuni eval to mover's perspective for display
            martuni_eval_cp: int | None = None
            if martuni_eval_white is not None:
                martuni_eval_cp = martuni_eval_white if mover == chess.WHITE else -martuni_eval_white

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
                martuni_eval_cp=martuni_eval_cp,
            )

        board.push(move)

    if skipped_fast:
        print(
            f"  ({skipped_fast} move(s) skipped in {game_id} — under {min_move_time}s)",
            file=sys.stderr,
        )


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


def collect_pgns(
    game_dir: Path,
    since: datetime | None,
) -> list[Path]:
    """Collect all PGN files in game_dir, optionally filtered by mtime."""
    pgns = sorted(game_dir.glob("*.pgn"))
    if since is not None:
        since_ts = since.timestamp()
        pgns = [p for p in pgns if p.stat().st_mtime >= since_ts]
    return pgns


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
        martuni = ""
        if b.martuni_eval_cp is not None:
            diff = b.martuni_eval_cp - b.eval_before_cp
            sign = "+" if diff >= 0 else ""
            martuni = f"  martuni={b.martuni_eval_cp:+d}cp(sf_diff={sign}{diff})"
        print(
            f"[{b.game_id}] {b.move_number}{'.' if b.side == 'white' else '...'} "
            f"{b.move_san}  loss={b.loss_cp}cp  phase={b.phase}  motifs={motifs}{best}{martuni}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "pgn",
        nargs="*",
        type=Path,
        help="PGN file(s) to analyze. Omit when using --game-dir.",
    )
    parser.add_argument(
        "--game-dir",
        type=Path,
        default=None,
        metavar="DIR",
        help="Directory with PGN files to analyze (e.g. ../lichess-bot/game_records/). "
             "Scans all *.pgn files. Combine with --since to filter by date.",
    )
    parser.add_argument(
        "--since",
        default=None,
        metavar="YYYY-MM-DD[THH:MM]",
        help="Only include PGN files modified at or after this UTC date/time. "
             "Example: --since 2026-04-12T16:38 (the SEE commit).",
    )
    parser.add_argument(
        "--losses-only",
        action="store_true",
        default=False,
        help="Only analyze games where the target player lost.",
    )
    parser.add_argument(
        "--player",
        default="Martuni",
        help="Only analyze moves played by this player (substring match on "
        "the White/Black header, case-insensitive). Default: 'Martuni'.",
    )
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
        "--min-movetime",
        type=float,
        default=0.0,
        metavar="SECS",
        help="Skip moves where the player spent less than SECS seconds (e.g. 0.3). "
             "Useful to ignore book moves and pre-moves. Requires %clk annotations in the PGN. "
             "Default: 0.0 (no filtering).",
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

    # --- Build file list ---
    pgn_paths: list[Path] = list(args.pgn)

    since: datetime | None = None
    if args.since:
        try:
            fmt = "%Y-%m-%dT%H:%M" if "T" in args.since else "%Y-%m-%d"
            since = datetime.strptime(args.since, fmt).replace(tzinfo=timezone.utc)
        except ValueError:
            print(f"error: --since '{args.since}' is not a valid date (use YYYY-MM-DD or YYYY-MM-DDTHH:MM)", file=sys.stderr)
            return 2

    if args.game_dir is not None:
        if not args.game_dir.is_dir():
            print(f"error: --game-dir '{args.game_dir}' is not a directory", file=sys.stderr)
            return 2
        discovered = collect_pgns(args.game_dir, since)
        print(f"info: found {len(discovered)} PGN(s) in {args.game_dir}" +
              (f" since {args.since}" if since else ""), file=sys.stderr)
        pgn_paths.extend(discovered)
    elif since is not None:
        # --since without --game-dir: filter the explicitly given files
        pgn_paths = [p for p in pgn_paths if p.stat().st_mtime >= since.timestamp()]

    if not pgn_paths:
        print("error: no PGN files to analyze. Use positional args or --game-dir.", file=sys.stderr)
        return 2

    for p in pgn_paths:
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
    skipped = 0
    skipped_wins = 0
    try:
        for game_id, game in iter_games(pgn_paths):
            target_colors = player_colors(game, args.player)
            if not target_colors:
                skipped += 1
                print(
                    f"skip: {game_id} — '{args.player}' not found in headers",
                    file=sys.stderr,
                )
                continue
            if args.losses_only and not martuni_lost(game, args.player, target_colors):
                skipped_wins += 1
                continue
            for blunder in analyze_game(
                engine, game, limit, args.threshold, game_id, target_colors,
                min_move_time=args.min_movetime,
            ):
                report.blunders.append(blunder)
    finally:
        engine.quit()

    if skipped:
        print(f"\n({skipped} game(s) skipped — no '{args.player}' header match)")
    if skipped_wins:
        print(f"({skipped_wins} game(s) skipped — not a loss, --losses-only active)")

    print_report(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
