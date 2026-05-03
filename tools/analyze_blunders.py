#!/usr/bin/env python3
"""Analyze PGN games with Stockfish, detect blunders, group by phase and motif."""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from dataclasses import asdict, dataclass, field
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


def count_king_zone_attackers(board: chess.Board, defender: chess.Color) -> int:
    """Count distinct enemy pieces attacking the defender king's 3x3 zone."""
    king_sq = board.king(defender)
    if king_sq is None:
        return 0

    zone = chess.SquareSet(chess.BB_KING_ATTACKS[king_sq]) | chess.SquareSet(chess.BB_SQUARES[king_sq])
    attackers: set[chess.Square] = set()
    for sq in zone:
        attackers.update(board.attackers(not defender, sq))
    return len(attackers)


def is_king_walk(
    board_before: chess.Board,
    board_after: chess.Board,
    move: chess.Move,
    mover: chess.Color,
) -> bool:
    """König marschiert im Mittelspiel/frühen Endspiel in die gegnerische Hälfte.

    Motivierend: 16...Kg4 im mochi_bot-Spiel (EY25JUSH). Der König verließ das
    Schach auf f5, hatte Kg6 (sicher, Rang 2 vom Heimrand) und Kg4 (Rang 4 vom
    Heimrand) zur Auswahl und lief in die Angreifer hinein.

    Heuristik:
      - Bewegte Figur muss der König sein
      - Zielrang ≥3 Reihen vom Heimrand entfernt (rank_dist ≥ 3)
      - Genug Nicht-Bauern-Material vom Gegner auf dem Brett (≥2000cp), sonst
        ist es ein legitimer Endspiel-König-Marsch.
    """
    piece = board_before.piece_at(move.from_square)
    if piece is None or piece.piece_type != chess.KING:
        return False

    # Gegnerisches Schwerfiguren-Material muss noch substantiell sein.
    # Schwelle 1500cp: deckt "2 Türme + Leichtfigur" (1300cp) knapp nicht ab,
    # aber "2 Türme + 2 Leichtfiguren" (1600cp) sehr wohl — das ist die
    # Mochi-EY25JUSH-Situation nach Damentausch. Unter 1500cp ist aktiver
    # König schon wertvoller als Sicherheit (KR+N-Endspiele etc.).
    enemy_npm = 0
    for pt in (chess.KNIGHT, chess.BISHOP, chess.ROOK, chess.QUEEN):
        enemy_npm += len(board_after.pieces(pt, not mover)) * PIECE_VALUES[pt]
    if enemy_npm < 1500:
        return False

    to_rank = chess.square_rank(move.to_square)
    # Heimrand: 0 für Weiß, 7 für Schwarz
    home = 0 if mover == chess.WHITE else 7
    rank_dist = abs(to_rank - home)
    return rank_dist >= 3


def is_exposed_king(board_after: chess.Board, mover: chess.Color) -> bool:
    """Statisch: König steht nach dem Zug in einer offenen 3×3-Zone.

    Ergänzend zu `king_safety` (das einen Sprung der Angreiferzahl verlangt):
    Diese Variante feuert, sobald mindestens 3 verschiedene gegnerische Figuren
    die King-Zone angreifen — unabhängig davon, ob vorher schon so viele waren.

    So werden Fälle erfasst, in denen schon mehrere Züge vorher der König offen
    stand und ein weiterer Zug die Stellung nicht verbessert hat.
    """
    return count_king_zone_attackers(board_after, mover) >= 3


def is_trade_down(
    board_before: chess.Board,
    board_after: chess.Board,
    move: chess.Move,
    info_after: chess.engine.InfoDict,
) -> bool:
    """Unsere Schlagfolge verliert netto Material.

    Wir schlagen eine Figur X, die beste Antwort des Gegners ist eine Rückschlagung
    auf demselben Feld mit einer Figur Y, wobei unser Opfer teurer war als X.

    Ergänzt `hangs_*` für Fälle, in denen unsere schlagende Figur nominell eine
    Deckung hatte — SEE-lite-Heuristik daneben liegen konnte, aber die Exchange-
    Bilanz trotzdem minus ist.
    """
    if not board_before.is_capture(move):
        return False

    # Wert, den unser Schlag gewinnt
    captured_on_ours = board_before.piece_at(move.to_square)
    if captured_on_ours is None:
        # En-passant: ein Bauer
        gained = PIECE_VALUES[chess.PAWN]
    else:
        gained = PIECE_VALUES[captured_on_ours.piece_type]

    # Beste Gegnerantwort
    pv = info_after.get("pv", [])
    if not pv:
        return False
    reply = pv[0]
    if reply.to_square != move.to_square:
        # Keine Rückschlagung auf unserem Zielfeld → dieser Motif-Prüfer greift nicht
        return False
    our_piece_now = board_after.piece_at(reply.to_square)
    if our_piece_now is None:
        return False
    lost = PIECE_VALUES[our_piece_now.piece_type]

    # Handelt es sich wirklich um eine verlierende Schlagfolge?
    # Netto < 0 heißt: wir haben die Exchange verloren.
    return (gained - lost) <= -100


def is_hanging(board: chess.Board, square: chess.Square) -> bool:
    """SEE-lite heuristic for pieces that are under-defended after a move."""
    piece = board.piece_at(square)
    if piece is None:
        return False
    attackers = [
        PIECE_VALUES[board.piece_at(s).piece_type]
        for s in board.attackers(not piece.color, square)
        if board.piece_at(s) is not None
    ]
    if not attackers:
        return False
    defenders = [
        PIECE_VALUES[board.piece_at(s).piece_type]
        for s in board.attackers(piece.color, square)
        if board.piece_at(s) is not None
    ]
    if not defenders:
        return True

    if len(attackers) > len(defenders):
        return True

    # If the exchange is numerically balanced, still tag obvious loose pieces
    # where the cheapest attacker wins material and the cheapest recapture is not cheap.
    attackers.sort()
    defenders.sort()
    min_attacker = attackers[0]
    min_defender = defenders[0]
    piece_val = PIECE_VALUES[piece.piece_type]
    return len(attackers) == len(defenders) and min_attacker < piece_val and min_attacker < min_defender


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

    # King safety degradation: more distinct enemy pieces attack our king zone after the move.
    attackers_before = count_king_zone_attackers(board_before, mover)
    attackers_after = count_king_zone_attackers(board_after, mover)
    if attackers_after >= 4 and attackers_after > attackers_before:
        motifs.append("king_safety")

    # König-Marsch ins Mittelspiel-Feuer (2026-04-22 eingeführt).
    # Fängt das Kg4-Muster aus dem mochi_bot-Spiel: in Stellungen mit vielen
    # gegnerischen Figuren wandert der König freiwillig (oder halb-erzwungen)
    # in die gegnerische Hälfte. Eigenständig von king_safety, weil der
    # Angreiferzahlen-Sprung nicht immer eintritt (wenige aber starke Angreifer).
    if is_king_walk(board_before, board_after, move, mover):
        motifs.append("king_walk")

    # Statische König-Exposition (2026-04-22 eingeführt).
    # Feuert ohne „Sprung"-Bedingung, sobald der König nach dem Zug in einer
    # offenen Zone steht. Ergänzt king_safety (das nur bei Eskalation anschlägt).
    if "king_safety" not in motifs and is_exposed_king(board_after, mover):
        motifs.append("exposed_king")

    # Verlierende Schlag-Folge (2026-04-22 eingeführt).
    # Greift, wenn unser Capture durch eine teurere Rückschlagung beantwortet
    # wird. Ergänzt hangs_*, weil die SEE-lite-Heuristik bei gedeckten
    # Figuren fälschlich „nicht hängend" sagen kann, die Gesamt-Exchange aber
    # trotzdem minus ist.
    if is_trade_down(board_before, board_after, move, info_after):
        motifs.append("trade_down")

    # Material swing without compensation: eval loss very large and no motif fired yet.
    if not motifs and (eval_before - eval_after) >= 300:
        motifs.append("positional_collapse")

    return motifs


def classify_time_control(tc_tag: str) -> str | None:
    """Lichess-Klassifikation per est = base + 40 * inc.

    Lichess kategorisiert Partien nach `est = base + 40·inc` (Sekunden):
    < 180 = bullet, < 480 = blitz, < 1500 = rapid, sonst classical.
    Korrespondenz (`-`), unbekannt (`*`/leer) und exotische Mehrphasen-
    Kontrollen (z. B. `15/40+30/60`) liefern `None` — der Aufrufer fällt
    dann auf `--min-movetime` zurück.
    """
    if not tc_tag or tc_tag in ("-", "*"):
        return None
    try:
        base, inc = tc_tag.split("+", 1)
        est = int(base) + 40 * int(inc)
    except ValueError:
        return None
    if est < 180:
        return "bullet"
    if est < 480:
        return "blitz"
    if est < 1500:
        return "rapid"
    return "classical"


def threshold_for_game(game: chess.pgn.Game, args: argparse.Namespace) -> tuple[float, str | None]:
    """Effektive Movetime-Untergrenze + Klasse für eine Partie.

    Hintergrund: Lichess-PGNs haben `[%clk H:MM:SS]` mit Sekunden-Auflösung,
    daher sind sub-Sekunden-Schwellen sinnlos. Die per-Klasse-Defaults sind
    bewusst `int`-typisiert. Wenn der `[TimeControl]`-Tag fehlt oder nicht
    parsbar ist, fällt die Funktion auf `--min-movetime` zurück (alter Pfad).
    """
    klass = classify_time_control(game.headers.get("TimeControl", ""))
    if klass is None:
        return float(args.min_movetime), None
    return float(getattr(args, f"min_movetime_{klass}")), klass


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
    tc_class: str | None = None,
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
        if tc_class is not None:
            # Schwelle ist int für die per-Klasse-Defaults (s.o.); ohne `:g`
            # würde 1.0 als "1.0s" geprintet, das verschleiert die Auflösung.
            thr_disp = f"{int(min_move_time)}" if float(min_move_time).is_integer() else f"{min_move_time}"
            print(
                f"  ({skipped_fast} move(s) skipped in {game_id} — {tc_class}, threshold {thr_disp}s)",
                file=sys.stderr,
            )
        else:
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


def load_state(path: Path) -> tuple[set[str], list[Blunder]]:
    """Inkrementellen Analysezustand aus JSON laden.

    Gibt (analyzed_pgns, blunders) zurück — analyzed_pgns ist die Menge der
    PGN-Dateinamen, die in früheren Läufen bereits verarbeitet wurden.
    """
    with path.open("r", encoding="utf-8") as fh:
        data = json.load(fh)
    analyzed = set(data.get("analyzed_pgns", []))
    blunders = [Blunder(**b) for b in data.get("blunders", [])]
    return analyzed, blunders


def save_state(path: Path, analyzed_pgns: set[str], blunders: list[Blunder]) -> None:
    """Inkrementellen Analysezustand in JSON schreiben (atomar per tmpfile)."""
    data = {
        "version": 1,
        "updated_at": datetime.now(timezone.utc).isoformat(),
        "analyzed_pgns": sorted(analyzed_pgns),
        "blunders": [asdict(b) for b in blunders],
    }
    # Schreibe zuerst in tmp-Datei, dann umbenennen — kein halb-geschriebener Zustand.
    tmp = path.with_suffix(".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        json.dump(data, fh, ensure_ascii=False, indent=2)
    tmp.replace(path)


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
        fen = f" fen={b.fen_before}"
        martuni = ""
        if b.martuni_eval_cp is not None:
            diff = b.martuni_eval_cp - b.eval_before_cp
            sign = "+" if diff >= 0 else ""
            martuni = f"  martuni={b.martuni_eval_cp:+d}cp(sf_diff={sign}{diff})"
        print(
            f"[{b.game_id}] {b.move_number}{'.' if b.side == 'white' else '...'} "
            f"{b.move_san}  loss={b.loss_cp}cp  phase={b.phase}  motifs={motifs}{best}{fen}{martuni}"
        )


def print_incremental_report(
    new_report: Report,
    cumulative_report: Report,
    new_pgn_count: int,
    total_pgn_count: int,
) -> None:
    """Batch-Ergebnis + kumulierte Zusammenfassung für inkrementelle Läufe."""
    now = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    sep = "=" * 60
    print(f"\n{sep}")
    print(f"  Batch: {now} — {new_pgn_count} neue PGN(s)")
    print(f"{sep}")

    if new_report.blunders:
        print_report(new_report)
    else:
        print("\nKeine neuen Blunder in diesem Batch.\n")

    total = len(cumulative_report.blunders)
    dsep = "─" * 60
    print(f"\n{dsep}")
    print(f"  Kumulativ: {total_pgn_count} PGN(s) analysiert, {total} Blunder gesamt")
    print(f"{dsep}")

    if not cumulative_report.blunders:
        return

    print("By phase:")
    for phase, blunders in sorted(cumulative_report.by_phase().items()):
        print(f"  {phase:<12} {len(blunders):>4}")
    print()

    print("By motif:")
    for motif, blunders in sorted(cumulative_report.by_motif().items(), key=lambda kv: -len(kv[1])):
        print(f"  {motif:<24} {len(blunders):>4}")
    print()


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
    # Per-Klasse-Schwellen sind int, weil Lichess-PGNs `[%clk H:MM:SS]`-
    # Sekundenauflösung haben — Zehntelsekunden kommen aus den PGNs nicht
    # heraus. Defaults sind die konservative Variante (siehe
    # docs/blunder-analyse.md): bestehende Stichproben werden nur leicht
    # strenger gefiltert als zuvor mit dem pauschalen 0.3s-Filter.
    parser.add_argument(
        "--min-movetime-bullet",
        type=int,
        default=0,
        metavar="SECS",
        help="Min. Bedenkzeit (Sekunden, integer) für Bullet-Partien (est < 180s). Default: 0.",
    )
    parser.add_argument(
        "--min-movetime-blitz",
        type=int,
        default=1,
        metavar="SECS",
        help="Min. Bedenkzeit (Sekunden, integer) für Blitz-Partien (180s ≤ est < 480s). Default: 1.",
    )
    parser.add_argument(
        "--min-movetime-rapid",
        type=int,
        default=3,
        metavar="SECS",
        help="Min. Bedenkzeit (Sekunden, integer) für Rapid-Partien (480s ≤ est < 1500s). Default: 3.",
    )
    parser.add_argument(
        "--min-movetime-classical",
        type=int,
        default=5,
        metavar="SECS",
        help="Min. Bedenkzeit (Sekunden, integer) für Classical-Partien (est ≥ 1500s). Default: 5.",
    )
    parser.add_argument(
        "--min-movetime",
        type=float,
        default=0.3,
        metavar="SECS",
        help="Fallback, wenn der [TimeControl]-Tag fehlt oder unparsbar ist (z.B. Korrespondenz '-'). "
             "Bei vorhandenem Tag greifen --min-movetime-{bullet,blitz,rapid,classical}. "
             "Default: 0.3.",
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
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        metavar="FILE",
        help=(
            "JSON-Zustandsdatei für inkrementelle Analyse. "
            "Existiert die Datei, werden bereits verarbeitete PGNs übersprungen "
            "und neue Blunder angehängt. Existiert sie nicht, wird sie neu angelegt. "
            "Kombinierbar mit --report (dann nur Anzeige, keine Analyse)."
        ),
    )
    parser.add_argument(
        "--report",
        action="store_true",
        default=False,
        help="Kumulierten Report aus --output FILE anzeigen, ohne neue Analyse zu starten.",
    )
    args = parser.parse_args()

    # --- --report-Modus: akkumulierten Bericht aus Zustandsdatei anzeigen ---
    if args.report:
        if args.output is None:
            print("error: --report requires --output FILE", file=sys.stderr)
            return 2
        if not args.output.exists():
            print(f"error: Zustandsdatei '{args.output}' nicht gefunden", file=sys.stderr)
            return 2
        analyzed_pgns_set, all_blunders = load_state(args.output)
        print(f"Kumulativ: {len(analyzed_pgns_set)} PGN(s) analysiert\n")
        print_report(Report(all_blunders))
        return 0

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

    for p in pgn_paths:
        if not p.exists():
            print(f"error: {p} not found", file=sys.stderr)
            return 2

    # --- Inkrementell: bereits analysierte PGNs herausfiltern ---
    analyzed_pgns: set[str] = set()
    existing_blunders: list[Blunder] = []
    prev_pgn_count = 0
    incremental = args.output is not None

    if incremental and args.output.exists():
        analyzed_pgns, existing_blunders = load_state(args.output)
        prev_pgn_count = len(analyzed_pgns)
        before_count = len(pgn_paths)
        pgn_paths = [p for p in pgn_paths if p.name not in analyzed_pgns]
        already_done = before_count - len(pgn_paths)
        if already_done:
            print(
                f"info: {already_done} PGN(s) bereits in Zustandsdatei, übersprungen "
                f"({len(pgn_paths)} neue verbleiben)",
                file=sys.stderr,
            )

    if not pgn_paths:
        if incremental:
            print("info: keine neuen PGN-Dateien — Zustandsdatei ist aktuell", file=sys.stderr)
            print(f"\nKumulativ: {prev_pgn_count} PGN(s), {len(existing_blunders)} Blunder\n")
            print_report(Report(existing_blunders))
        else:
            print("error: no PGN files to analyze. Use positional args or --game-dir.", file=sys.stderr)
            return 2
        return 0

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

    new_blunders: list[Blunder] = []
    processed_pgns: set[str] = set()
    skipped = 0
    skipped_wins = 0
    try:
        # Datei für Datei iterieren damit wir nach jeder Datei den Zustand
        # sichern können — Absturz mitten im Lauf verliert dann nur den
        # Fortschritt der laufenden Datei, nicht alles.
        for pgn_path in pgn_paths:
            for game_id, game in iter_games([pgn_path]):
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
                effective_min_movetime, tc_class = threshold_for_game(game, args)
                for blunder in analyze_game(
                    engine, game, limit, args.threshold, game_id, target_colors,
                    min_move_time=effective_min_movetime,
                    tc_class=tc_class,
                ):
                    new_blunders.append(blunder)

            processed_pgns.add(pgn_path.name)

            if incremental:
                save_state(
                    args.output,
                    analyzed_pgns | processed_pgns,
                    existing_blunders + new_blunders,
                )
    finally:
        engine.quit()

    if skipped:
        print(f"\n({skipped} game(s) skipped — no '{args.player}' header match)", file=sys.stderr)
    if skipped_wins:
        print(f"({skipped_wins} game(s) skipped — not a loss, --losses-only active)", file=sys.stderr)

    if incremental:
        all_blunders = existing_blunders + new_blunders
        print_incremental_report(
            new_report=Report(new_blunders),
            cumulative_report=Report(all_blunders),
            new_pgn_count=len(processed_pgns),
            total_pgn_count=prev_pgn_count + len(processed_pgns),
        )
    else:
        print_report(Report(new_blunders))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
