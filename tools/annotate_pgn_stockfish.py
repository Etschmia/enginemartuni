#!/usr/bin/env python3
"""Eine einzelne PGN-Partie mit Stockfish analysieren und kritische Züge kommentieren.

  - erwartet eine PGN-Datei mit genau einer Partie
  - bricht bei Mehrpartien-PGN oder korruptem/unerwartetem PGN-Token ab
  - analysiert mit Stockfish auf depth 17
  - vergleicht Bestmove-Score gegen den erzwungen analysierten gespielten Zug
  - hängt bei Verlust > 80 cp einen Kommentar an
  - erhält bestehende Kommentare wie [%clk ...]

  Standardmäßig überschreibt es die Eingabedatei atomar:

  .venv/bin/python tools/annotate_pgn_stockfish.py partie.pgn

  Optional separat ausgeben:

  .venv/bin/python tools/annotate_pgn_stockfish.py partie.pgn --output partie_annotiert.pgn

"""

from __future__ import annotations

import argparse
import io
import os
import re
import select
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import chess
import chess.pgn


MATE_SCORE_CP = 100_000
TAG_LINE_RE = re.compile(r'\[[A-Za-z0-9][A-Za-z0-9_+#=:-]*\s+"(?:\\.|[^"\\\r\n])*"\]\s*\Z')
MOVE_NUMBER_RE = re.compile(r"\d+\.(?:\.\.)?")
NAG_RE = re.compile(r"\$[0-9]+")
RESULT_RE = re.compile(r"1-0|0-1|1/2-1/2|\*")
ANNOTATION_RE = re.compile(r"!!|\?\?|!\?|\?!|!|\?")
SAN_RE = re.compile(
    r"(?:"
    r"[NBKRQ]?[a-h]?[1-8]?[\-x]?[a-h][1-8](?:=?[nbrqkNBRQK])?"
    r"|[PNBRQK]?@[a-h][1-8]"
    r"|--"
    r"|Z0"
    r"|0000"
    r"|@@@@"
    r"|O-O(?:-O)?"
    r"|0-0(?:-0)?"
    r")(?:[+#]+)?"
)


@dataclass(frozen=True)
class EngineScore:
    """Stockfish-Score aus Sicht der Seite, die am Brett am Zug ist."""

    kind: str
    value: int

    def to_cp(self) -> int:
        """Engine-Score in Centipawns umrechnen."""
        if self.kind == "mate":
            # Mattbewertungen werden stark gekappt, damit sie mit cp-Werten vergleichbar bleiben.
            return MATE_SCORE_CP - abs(self.value) if self.value > 0 else -MATE_SCORE_CP + abs(self.value)
        return self.value

    def format(self) -> str:
        """Score kurz fuer den PGN-Kommentar formatieren."""
        if self.kind == "mate":
            return f"#{self.value}" if self.value > 0 else f"#-{abs(self.value)}"
        return f"{self.value / 100:+.2f}"


@dataclass(frozen=True)
class AnalysisResult:
    """Das Minimum, das wir pro Stockfish-Suche brauchen."""

    best_move: chess.Move
    score: EngineScore


class Stockfish:
    """Kleine UCI-Anbindung ohne python-chess SimpleEngine/asyncio."""

    def __init__(self, command: str, timeout: float) -> None:
        self.command = command
        self.timeout = timeout
        self.process = subprocess.Popen(
            [command],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            bufsize=0,
        )
        if self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError("Stockfish konnte nicht mit Pipes gestartet werden.")
        self.stdin = self.process.stdin
        self.stdout = self.process.stdout
        self._stdout_buffer = b""
        try:
            self._initialize()
        except Exception:
            self.close()
            raise

    def __enter__(self) -> Stockfish:
        return self

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.close()

    def _send(self, command: str) -> None:
        """Einen UCI-Befehl an Stockfish schicken."""
        if self.process.poll() is not None:
            raise RuntimeError("Stockfish-Prozess ist beendet.")
        self.stdin.write(f"{command}\n".encode("utf-8"))
        self.stdin.flush()

    def _readline_until(self, deadline: float) -> str:
        """Eine Zeile lesen, aber nicht endlos haengen bleiben."""
        while True:
            if self.process.poll() is not None:
                raise RuntimeError("Stockfish-Prozess ist unerwartet beendet.")

            if b"\n" in self._stdout_buffer:
                line, self._stdout_buffer = self._stdout_buffer.split(b"\n", 1)
                return line.decode("utf-8", errors="replace").strip()

            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"Stockfish antwortet seit {self.timeout:g}s nicht.")

            ready, _, _ = select.select([self.stdout], [], [], remaining)
            if not ready:
                raise TimeoutError(f"Stockfish antwortet seit {self.timeout:g}s nicht.")

            chunk = os.read(self.stdout.fileno(), 4096)
            if chunk == b"":
                raise RuntimeError("Stockfish-Ausgabe wurde geschlossen.")
            self._stdout_buffer += chunk

    def _wait_for(self, expected: str) -> None:
        """Bis zu einer bestimmten UCI-Antwort lesen."""
        deadline = time.monotonic() + self.timeout
        while True:
            if self._readline_until(deadline) == expected:
                return

    def _initialize(self) -> None:
        """UCI-Handshake ausfuehren und warten, bis Stockfish bereit ist."""
        self._send("uci")
        self._wait_for("uciok")
        self._send("isready")
        self._wait_for("readyok")

    @staticmethod
    def _parse_score(line: str) -> EngineScore | None:
        """Aus einer UCI-info-Zeile die letzte Score-Angabe extrahieren."""
        parts = line.split()
        for index, token in enumerate(parts):
            if token != "score" or index + 2 >= len(parts):
                continue
            kind = parts[index + 1]
            value = parts[index + 2]
            if kind in {"cp", "mate"}:
                return EngineScore(kind, int(value))
        return None

    def analyse(self, board: chess.Board, depth: int, root_move: chess.Move | None = None) -> AnalysisResult:
        """Eine Stellung analysieren, optional auf genau einen Root-Zug beschraenkt."""
        self._send(f"position fen {board.fen()}")
        if root_move is None:
            self._send(f"go depth {depth}")
        else:
            # Der gespielte Zug wird ueber UCI searchmoves erzwungen.
            self._send(f"go depth {depth} searchmoves {root_move.uci()}")

        score: EngineScore | None = None
        best_move: chess.Move | None = None
        deadline = time.monotonic() + self.timeout

        while True:
            line = self._readline_until(deadline)
            if line.startswith("info "):
                parsed_score = self._parse_score(line)
                if parsed_score is not None:
                    score = parsed_score
            elif line.startswith("bestmove "):
                parts = line.split()
                if len(parts) < 2 or parts[1] == "(none)":
                    raise RuntimeError("Stockfish hat keinen Bestmove geliefert.")
                best_move = chess.Move.from_uci(parts[1])
                break

        if score is None:
            raise RuntimeError("Stockfish hat keinen verwertbaren Score geliefert.")
        return AnalysisResult(best_move, score)

    def close(self) -> None:
        """Stockfish sauber beenden, falls der Prozess noch laeuft."""
        if self.process.poll() is not None:
            return
        try:
            self._send("quit")
            self.process.wait(timeout=2)
        except (BrokenPipeError, RuntimeError, subprocess.TimeoutExpired):
            self.process.kill()
            self.process.wait(timeout=2)


def append_comment(existing: str, addition: str) -> str:
    """Vorhandene Kommentare exakt erhalten und den neuen Kommentar nur anhaengen."""
    if not existing:
        return addition
    separator = "" if existing.endswith((" ", "\n", "\t")) else " "
    return f"{existing}{separator}{addition}"


def line_and_column(text: str, index: int) -> tuple[int, int]:
    """Menschenlesbare Position fuer Fehlermeldungen berechnen."""
    line = text.count("\n", 0, index) + 1
    line_start = text.rfind("\n", 0, index)
    column = index + 1 if line_start == -1 else index - line_start
    return line, column


def validate_pgn_lexically(text: str) -> None:
    """Unbekannte PGN-Tokens finden, die python-chess sonst tolerant ueberspringt."""
    index = 0
    line_start = True
    token_res = (MOVE_NUMBER_RE, RESULT_RE, NAG_RE, ANNOTATION_RE, SAN_RE)

    while index < len(text):
        char = text[index]

        if char in "\r\n":
            index += 1
            line_start = True
            continue
        if char.isspace():
            index += 1
            continue

        if line_start and char == "%":
            # PGN-Escape-Zeilen beginnen in Spalte 1 mit Prozent.
            newline = text.find("\n", index)
            index = len(text) if newline == -1 else newline + 1
            line_start = True
            continue

        if line_start and char == "[":
            newline = text.find("\n", index)
            line_end = len(text) if newline == -1 else newline
            line = text[index:line_end]
            if not TAG_LINE_RE.match(line):
                line_no, column = line_and_column(text, index)
                raise ValueError(f"malformed PGN-Header bei Zeile {line_no}, Spalte {column}")
            index = line_end
            line_start = False
            continue

        line_start = False

        if char == ";":
            newline = text.find("\n", index)
            index = len(text) if newline == -1 else newline + 1
            line_start = True
            continue

        if char == "{":
            close_index = text.find("}", index + 1)
            if close_index == -1:
                line_no, column = line_and_column(text, index)
                raise ValueError(f"nicht geschlossener PGN-Kommentar bei Zeile {line_no}, Spalte {column}")
            index = close_index + 1
            continue

        if char in "()":
            index += 1
            continue

        for token_re in token_res:
            match = token_re.match(text, index)
            if match is not None:
                index = match.end()
                break
        else:
            line_no, column = line_and_column(text, index)
            snippet = text[index : index + 20].splitlines()[0]
            raise ValueError(f"unerwartetes PGN-Token bei Zeile {line_no}, Spalte {column}: {snippet!r}")


def read_single_game(path: Path) -> chess.pgn.Game:
    """PGN lesen und sicherstellen, dass genau eine fehlerfreie Partie enthalten ist."""
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        raise ValueError(f"PGN ist nicht gueltig UTF-8-kodiert: {exc}") from exc

    validate_pgn_lexically(text)

    handle = io.StringIO(text)
    game = chess.pgn.read_game(handle)
    if game is None:
        raise ValueError("keine Partie gefunden")

    # python-chess sammelt PGN-Parserfehler in game.errors statt hart abzubrechen.
    if game.errors:
        details = "; ".join(str(error) for error in game.errors[:3])
        raise ValueError(f"PGN ist korrupt: {details}")

    second_game = chess.pgn.read_game(handle)
    if second_game is not None:
        raise ValueError("PGN enthaelt mehr als eine Partie")

    return game


def build_comment(
    depth: int,
    move_san: str,
    best_san: str,
    best_score: EngineScore,
    played_score: EngineScore,
    loss_cp: int,
) -> str:
    """Den eigentlichen Hinweistext fuer einen schlechten Zug erzeugen."""
    return (
        f"Stockfish depth {depth}: besser war {best_san} "
        f"({best_score.format()} statt {played_score.format()} "
        f"nach {move_san}, Verlust {loss_cp / 100:.2f})."
    )


def annotate_game(
    engine: Stockfish,
    game: chess.pgn.Game,
    depth: int,
    threshold_cp: int,
) -> int:
    """Alle Hauptvariantenzuege analysieren und bei groesseren Verlusten kommentieren."""
    board = game.board()
    node: chess.pgn.GameNode = game
    annotated = 0
    ply = 0

    while node.variations:
        next_node = node.variations[0]
        move = next_node.move
        move_san = board.san(move)
        ply += 1

        print(f"Analysiere Zug {ply}: {move_san}", file=sys.stderr)

        # Erst die Stellung vor dem Zug frei analysieren: daraus kommen Bestmove und Bestscore.
        best_result = engine.analyse(board, depth)
        best_move = best_result.best_move

        # Wenn der gespielte Zug bereits Stockfishs Bestmove ist, kann kein Verlust entstehen.
        if move != best_move:
            # Danach dieselbe Ausgangsstellung analysieren, aber den gespielten Zug erzwingen.
            played_result = engine.analyse(board, depth, root_move=move)

            loss_cp = best_result.score.to_cp() - played_result.score.to_cp()
            if loss_cp > threshold_cp:
                best_san = board.san(best_move)
                comment = build_comment(
                    depth,
                    move_san,
                    best_san,
                    best_result.score,
                    played_result.score,
                    loss_cp,
                )
                next_node.comment = append_comment(next_node.comment, comment)
                annotated += 1

        board.push(move)
        node = next_node

    return annotated


def write_game(game: chess.pgn.Game, output_path: Path) -> None:
    """Annotierte PGN atomar schreiben, damit bei Fehlern keine halbe Datei bleibt."""
    exporter = chess.pgn.StringExporter(headers=True, variations=True, comments=True)
    text = game.accept(exporter)
    tmp_path = output_path.with_name(f".{output_path.name}.tmp")
    tmp_path.write_text(f"{text}\n", encoding="utf-8")
    tmp_path.replace(output_path)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Analysiert genau eine PGN-Partie mit Stockfish depth 17 und "
            "kommentiert Zuege, die mehr als 80 cp schlechter als der Bestmove sind."
        )
    )
    parser.add_argument("pgn", help="PGN-Datei mit genau einer Partie")
    parser.add_argument(
        "-o",
        "--output",
        help="Ausgabedatei; ohne Angabe wird die Eingabedatei ueberschrieben",
    )
    parser.add_argument(
        "--engine",
        default=os.environ.get("STOCKFISH", "stockfish"),
        help="Pfad zur Stockfish-Binaerdatei (Standard: STOCKFISH oder 'stockfish')",
    )
    parser.add_argument(
        "--depth",
        type=int,
        default=17,
        help="Suchtiefe fuer beide Vergleiche (Standard: 17)",
    )
    parser.add_argument(
        "--threshold-cp",
        type=int,
        default=80,
        help="Kommentarschwelle in Centipawns; kommentiert wird nur bei Verlust > Schwelle",
    )
    parser.add_argument(
        "--engine-timeout",
        type=float,
        default=300.0,
        help="Maximale Sekunden pro Stockfish-Antwort/Suche (Standard: 300)",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    pgn_path = Path(args.pgn)
    output_path = Path(args.output) if args.output else pgn_path

    if args.depth <= 0:
        print("Fehler: --depth muss groesser als 0 sein.", file=sys.stderr)
        return 1
    if args.threshold_cp < 0:
        print("Fehler: --threshold-cp darf nicht negativ sein.", file=sys.stderr)
        return 1
    if args.engine_timeout <= 0:
        print("Fehler: --engine-timeout muss groesser als 0 sein.", file=sys.stderr)
        return 1

    if not pgn_path.exists():
        print(f"Fehler: PGN-Datei nicht gefunden: {pgn_path}", file=sys.stderr)
        return 1

    try:
        game = read_single_game(pgn_path)
    except ValueError as exc:
        print(f"Fehler: {exc}", file=sys.stderr)
        return 1

    try:
        with Stockfish(args.engine, args.engine_timeout) as engine:
            annotated = annotate_game(engine, game, args.depth, args.threshold_cp)
    except FileNotFoundError:
        print(f"Fehler: Stockfish nicht gefunden: {args.engine}", file=sys.stderr)
        return 1
    except (OSError, RuntimeError, TimeoutError, ValueError) as exc:
        print(f"Fehler: Analyse abgebrochen: {exc}", file=sys.stderr)
        return 1

    try:
        write_game(game, output_path)
    except OSError as exc:
        print(f"Fehler: Konnte PGN nicht schreiben: {exc}", file=sys.stderr)
        return 1

    print(f"Fertig: {annotated} Kommentar(e) hinzugefuegt -> {output_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
