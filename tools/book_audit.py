#!/usr/bin/env python3
"""Eroeffnungsbuch-Audit ueber Martunis Lichess-Partien (nur Standard).

Grundlage fuer docs/eigenes-eroeffnungsbuch.md (30.09.2026).

Fuer jede Partie wird geprueft, ob Martuni in Stellungen, die in den
Polyglot-Buechern stehen, auch einen Buchzug gespielt hat ("befolgt") oder
davon abgewichen ist ("abgewichen" = Buch war nicht aktiv). Ausgewertet
wird je Zeitraum und je Eroeffnungslinie (erste zwei Halbzuege), jeweils
gegen den aus der Elo-Differenz erwarteten Score.

Zeitraeume: A vor 06.09.2026 (Buch aktiv), B 06.09.–24.09.2026 16:00
(fremde .env in ~/lichess-bot verdeckte BOOK_FILES -> kein Buch, Fix
845708b), C danach (Buch wieder aktiv).

Aufruf aus dem Repo-Root:  nice -n 19 python3 tools/book_audit.py
Rechenlast: nur PGN-Parsing, ein Kern, ~1 Minute.
"""
import collections
import glob
import math
import os

import chess
import chess.pgn
import chess.polyglot

BOOKS = ["martuni_patches.bin", "gm2001.bin", "komodo.bin", "rodent.bin"]
GAME_DIRS = ["~/lichess-bot/game_records", "~/lichess-bot/game_archiv"]
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
    return 1 / (1 + 10 ** (diff / 400))


def elo(p):
    p = min(max(p, 1e-3), 1 - 1e-3)
    return -400 * math.log10(1 / p - 1)


def period(stamp):
    if stamp < NO_BOOK_FROM:
        return "A vor 06.09. (Buch)"
    if stamp < NO_BOOK_UNTIL:
        return "B 06.–24.09. (ohne Buch)"
    return "C nach Fix (Buch)"


def main():
    files = []
    for d in GAME_DIRS:
        files += glob.glob(os.path.join(os.path.expanduser(d), "*.pgn"))

    per = collections.defaultdict(collections.Counter)
    lines = collections.defaultdict(lambda: [0, 0.0, 0.0])
    seen = set()
    for f in files:
        try:
            g = chess.pgn.read_game(open(f, encoding="utf-8", errors="replace"))
        except Exception:
            continue
        if g is None:
            continue
        h = g.headers
        if h.get("Variant", "Standard") not in ("Standard", "") or "FEN" in h:
            continue
        if h.get("Site") in seen:
            continue
        seen.add(h.get("Site"))
        if h.get("White") == "Martuni":
            col = chess.WHITE
        elif h.get("Black") == "Martuni":
            col = chess.BLACK
        else:
            continue
        res = h.get("Result")
        if res not in ("1-0", "0-1", "1/2-1/2"):
            continue
        score = 0.5 if res == "1/2-1/2" else float((res == "1-0") == (col == chess.WHITE))
        try:
            opp = int(h.get("BlackElo" if col else "WhiteElo"))
            me = int(h.get("WhiteElo" if col else "BlackElo"))
            diff = opp - me
        except (TypeError, ValueError):
            diff = 0
        stamp = h.get("UTCDate", h.get("Date", "")).replace(".", "-") + " " + h.get("UTCTime", "00:00:00")
        p = period(stamp)

        board = g.board()
        mode, exit_ply, first = "befolgt", None, []
        for ply, mv in enumerate(g.mainline_moves()):
            if ply < 2:
                first.append(board.san(mv))
            if board.turn == col and exit_ply is None:
                bm = book_moves(board)
                if bm is None:
                    exit_ply = ply
                elif mv not in bm:
                    mode, exit_ply = "abgewichen", ply
            board.push(mv)

        c = per[p]
        c["n"] += 1
        c["s"] += score
        c["e"] += expected(diff)
        c[mode] += 1
        c["exit"] += exit_ply if exit_ply is not None else board.ply()
        key = ("W " if col else "B ") + " ".join(first) + ("  [ohne]" if p.startswith("B") else "  [Buch]")
        lines[key][0] += 1
        lines[key][1] += score
        lines[key][2] += expected(diff)

    print("Zeitraeume (Score gegen Elo-Erwartung):")
    for p in sorted(per):
        c = per[p]
        n = c["n"]
        print(f"  {p:26s} n={n:4d}  Score {100*c['s']/n:5.1f} %  erwartet {100*c['e']/n:5.1f} %  "
              f"Perf {elo(c['s']/n)-elo(c['e']/n):+4.0f} Elo  befolgt {c['befolgt']:4d}  "
              f"abgewichen {c['abgewichen']:4d}  Ø Buchausgang Halbzug {c['exit']/n:4.1f}")
    print("\nLinien (erste zwei Halbzuege, n >= 20):")
    for k, (n, s, e) in sorted(lines.items()):
        if n >= 20:
            print(f"  {k:26s} n={n:4d}  Score {100*s/n:5.1f} %  erwartet {100*e/n:5.1f} %  Delta {100*(s-e)/n:+5.1f}")


if __name__ == "__main__":
    main()
