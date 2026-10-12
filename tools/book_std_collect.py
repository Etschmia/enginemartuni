#!/usr/bin/env python3
"""Phase 2 des Eigenbuchs (docs/eigenes-eroeffnungsbuch.md, Abschnitt 11):
Martunis live gespielte Buchzuege im Standardschach sammeln.

Das neue Standardbuch faengt NICHT bei Zug 1 neu an (Tobias-Entscheid
12.10.2026). Es uebernimmt die Zugfolgen, die Martuni live aus den fremden
Buechern (martuni_patches, gm2001, komodo, rodent) gespielt hat, samt
Ergebnis. Schlechte Zuege streichen spaeter book_std_veto.py (Stockfish)
und book_std_weights.py (Score-Filter).

Ein Buchzug ist ein Martuni-Zug in einer Stellung, die das erste Buch mit
Treffer kennt (wie BookSet::probe in der Engine), und der Zug steht dort
auch drin. Ab dem ersten Nicht-Buchzug zaehlt die Partie nicht mehr.
Ausgelassen: 06.09.–24.09.2026 16:00 (live ohne Buch, Fix 845708b),
Partien mit FEN-Start, Varianten, abgebrochene Partien ohne Ergebnis.

Eingabe: PGN-Dateien (Default book_work/std_lichess_all.pgn, der komplette
Lichess-Export von BOT Martuni; doppelte GameIds werden einmal gezaehlt).
Ausgabe: book_work/std_positions.json
  {"games", "positions": [{"fen" (EPD), "ply", "moves_from_start": [uci...],
    "played": {uci: {"n", "score", "exp"}}}]}
  moves_from_start = Zugfolge der ersten Partie, die die Stellung erreicht hat
  (bei Zugumstellung ist das nur einer von mehreren Wegen, die Stellung ist
  ueber den EPD-Schluessel trotzdem nur einmal drin).

Aufruf aus dem Repo-Root:  nice -n 19 python3 tools/book_std_collect.py [pgn ...]
Rechenlast: nur PGN-Parsing, ein Kern, einige Minuten.
"""
import collections
import json
import sys

import chess
import chess.pgn
import chess.polyglot

BOOKS = ["martuni_patches.bin", "gm2001.bin", "komodo.bin", "rodent.bin"]
NO_BOOK_FROM = "2026-09-06"
NO_BOOK_UNTIL = "2026-09-24 16:00"

readers = [chess.polyglot.open_reader("src/polyglot/" + b) for b in BOOKS]


def book_moves(board):
    """Zuege des ersten Buchs mit Treffer (wie BookSet::probe in der Engine)."""
    for r in readers:
        moves = {e.move for e in r.find_all(board)}
        if moves:
            return moves
    return None


def expected(diff):
    """Erwarteter Score bei Elo-Differenz diff = Gegner - Martuni."""
    return 1 / (1 + 10 ** (diff / 400))


def games(paths):
    for p in paths:
        with open(p, encoding="utf-8", errors="replace") as fh:
            while True:
                g = chess.pgn.read_game(fh)
                if g is None:
                    break
                yield g


def main():
    paths = sys.argv[1:] or ["book_work/std_lichess_all.pgn"]
    pos = {}
    seen = set()
    n_games = 0
    for g in games(paths):
        h = g.headers
        if h.get("Variant", "Standard") not in ("Standard", "") or "FEN" in h:
            continue
        gid = h.get("GameId") or h.get("Site")
        if gid in seen:
            continue
        seen.add(gid)
        if h.get("White") == "Martuni":
            col = chess.WHITE
        elif h.get("Black") == "Martuni":
            col = chess.BLACK
        else:
            continue
        res = h.get("Result")
        if res not in ("1-0", "0-1", "1/2-1/2"):
            continue
        stamp = h.get("UTCDate", "").replace(".", "-") + " " + h.get("UTCTime", "")
        if NO_BOOK_FROM <= stamp < NO_BOOK_UNTIL:
            continue
        score = 0.5 if res == "1/2-1/2" else float((res == "1-0") == (col == chess.WHITE))
        try:
            diff = int(h.get("BlackElo" if col else "WhiteElo")) - int(h.get("WhiteElo" if col else "BlackElo"))
        except (TypeError, ValueError):
            diff = 0
        exp = expected(diff)
        n_games += 1

        board = g.board()
        seq = []
        for mv in g.mainline_moves():
            if board.turn == col:
                bm = book_moves(board)
                if bm is None or mv not in bm:
                    break
                key = board.epd()
                p = pos.get(key)
                if p is None:
                    p = pos[key] = {"fen": key, "ply": board.ply(), "moves_from_start": list(seq),
                                    "played": collections.defaultdict(lambda: {"n": 0, "score": 0.0, "exp": 0.0})}
                st = p["played"][mv.uci()]
                st["n"] += 1
                st["score"] += score
                st["exp"] += exp
            seq.append(mv.uci())
            board.push(mv)

    out = sorted(pos.values(), key=lambda p: -sum(v["n"] for v in p["played"].values()))
    for p in out:
        p["played"] = {m: {"n": v["n"], "score": v["score"], "exp": round(v["exp"], 4)}
                       for m, v in p["played"].items()}
    json.dump({"games": n_games, "books": BOOKS, "positions": out},
              open("book_work/std_positions.json", "w"), indent=1)
    n_moves = sum(len(p["played"]) for p in out)
    print(f"{n_games} Partien -> {len(out)} Stellungen, {n_moves} Buchzuege -> book_work/std_positions.json")


if __name__ == "__main__":
    main()
