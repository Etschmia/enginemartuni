#!/usr/bin/env python3
"""Schritt 1 des Eigenbuchs (docs/eigenes-eroeffnungsbuch.md, Abschnitt 8):
haeufige Fruehstellungen einer Variante aus Martunis Lichess-Partien sammeln.

Nur Stellungen mit Martuni am Zug. Pro Stellung: Haeufigkeit, die gespielten
Zuege mit Martunis Ergebnis (Punkte aus Martuni-Sicht) und die Zugfolge ab
Grundstellung (fuer `position startpos moves ...` an der Engine).

Aufruf: python3 tools/book_collect.py 3check|koth [--max-ply 16] [--min-count 3]
Ausgabe: book_work/<variante>_positions.json

Seit 01.10.2026 zusaetzlich `exp` je Stellung und Zug: die aus der
Elo-Differenz erwarteten Punkte (Summe ueber die Partien), damit die
Gewichtsformel die Gegnerstaerke beruecksichtigt (tools/book_weights.py).
"""
import argparse, collections, glob, json, os
import chess, chess.pgn, chess.variant

VARIANTS = {
    "3check": ("Three-check", chess.variant.ThreeCheckBoard),
    "koth": ("King of the Hill", chess.variant.KingOfTheHillBoard),
}

def expected(diff):
    """Erwartete Punkte bei Elo-Differenz diff = Gegner - Martuni."""
    return 1 / (1 + 10 ** (diff / 400))

def key(board):
    # FEN ohne Zugzaehler; bei Three-check enthaelt sie die Schach-Zaehler.
    return " ".join(board.fen().split(" ")[:-2])

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("variant", choices=VARIANTS)
    ap.add_argument("--max-ply", type=int, default=16)
    ap.add_argument("--min-count", type=int, default=3)
    a = ap.parse_args()
    name, cls = VARIANTS[a.variant]
    files = glob.glob(os.path.expanduser("~/lichess-bot/game_records/*.pgn")) + \
            glob.glob(os.path.expanduser("~/lichess-bot/game_archiv/*.pgn"))
    pos = {}
    seen = set(); games = 0
    for f in files:
        with open(f, encoding="utf-8", errors="replace") as fh:
            head = fh.read(2000)
            if f'[Variant "{name}"]' not in head:
                continue
            fh.seek(0)
            g = chess.pgn.read_game(fh)
        if g is None or "FEN" in g.headers or g.headers.get("Site") in seen:
            continue
        h = g.headers
        seen.add(h.get("Site"))
        col = chess.WHITE if h.get("White") == "Martuni" else chess.BLACK if h.get("Black") == "Martuni" else None
        if col is None or h.get("Result") not in ("1-0", "0-1", "1/2-1/2"):
            continue
        res = h["Result"]
        score = 0.5 if res == "1/2-1/2" else float((res == "1-0") == (col == chess.WHITE))
        try:
            exp = expected(int(h.get("BlackElo" if col else "WhiteElo")) - int(h.get("WhiteElo" if col else "BlackElo")))
        except (TypeError, ValueError):
            exp = 0.5
        games += 1
        b = cls(); moves = []
        for ply, mv in enumerate(g.mainline_moves()):
            if ply >= a.max_ply:
                break
            if b.turn == col:
                k = key(b)
                p = pos.setdefault(k, {"fen": k, "ply": ply, "moves_from_start": list(moves),
                                       "count": 0, "score": 0.0, "exp": 0.0, "played": {}})
                p["count"] += 1; p["score"] += score; p["exp"] += exp
                st = p["played"].setdefault(mv.uci(), {"n": 0, "score": 0.0, "exp": 0.0})
                st["n"] += 1; st["score"] += score; st["exp"] += exp
            moves.append(mv.uci()); b.push(mv)
    sel = sorted((p for p in pos.values() if p["count"] >= a.min_count), key=lambda p: (p["ply"], -p["count"]))
    out = f"book_work/{a.variant}_positions.json"
    json.dump({"variant": a.variant, "games": games, "max_ply": a.max_ply, "min_count": a.min_count,
               "positions": sel}, open(out, "w"), indent=1)
    print(f"{name}: {games} Partien, {len(pos)} Stellungen mit Martuni am Zug (ply<{a.max_ply}), "
          f"{len(sel)} mit >= {a.min_count} Vorkommen -> {out}")
    byply = collections.Counter(p["ply"] for p in sel)
    print("  je Halbzug:", dict(sorted(byply.items())))

if __name__ == "__main__":
    main()
