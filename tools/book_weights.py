#!/usr/bin/env python3
"""Schritt 4 des Eigenbuchs (docs/eigenes-eroeffnungsbuch.md, Abschnitt 10):
Gewichte je Buchzug berechnen (Formel: Tobias-Entscheid 01.10.2026).

Nur Zuege, die das Stockfish-Veto ueberstanden haben (book_veto.py,
Schwelle je Variante), und nur Stellungen mit mindestens zwei solchen Zuegen.

    roh  = Q_SF * Q_M * L
    Q_SF = exp(-SF-Verlust / T)              Abstand zum Stockfish-Bestzug
    Q_M  = exp(-min(M-Abstand, 300) / T)     Abstand zu Martunis bestem Buchzug
    L    = 2 * (0.5 + (Punkte - Erwartet) / (n + k)),  k = 4,  begrenzt auf [0.05, 2]
    T    = Veto-Schwelle der Variante (Three-check 100, KotH 60)

L ist der geglaettete Live-Score mit Elo-Korrektur: ohne Live-Partien 1,
sonst zaehlt, wie viel Martuni ueber oder unter der Elo-Erwartung gepunktet
hat (k = 4 Pseudo-Partien auf Erwartungsniveau). Die Untergrenze 0,05 haelt
L positiv; der Mindestanteil sorgt ohnehin fuer Abwechslung.

Anteil = roh / Summe, jeder Zug mindestens 10 %; Polyglot-Gewicht =
Anteil * 1000 (gerundet, mindestens 1).

Live-Daten kommen frisch aus book_work/<variante>_positions.json
(tools/book_collect.py), Analyse und Veto aus *_analysis.json / *_veto.json.

Aufruf: python3 tools/book_weights.py 3check|koth
Ausgabe:
  book_work/<variante>_book.json  — alle Zwischenwerte zum Nachvollziehen:
    {"variant", "formula": {...}, "positions": [
       {"fen", "moves_from_start": [uci...], "moves": [{"uci", "san", "weight", "share", ...}]}]}
  book_work/<variante>_book.txt   — Eingabe fuer bookbuild (Rust), eine Zeile
    pro Buchzug, ohne JSON-Parser lesbar:
      <Zugfolge ab Grundstellung, UCI, Leerzeichen-getrennt> | <Buchzug UCI> <Gewicht>
    Grundstellung = leere Zugfolge (Zeile beginnt mit "| "). Rochade steht in
    Standard-UCI (e1g1); "#"-Zeilen sind Kommentare.
"""
import argparse, json, math, sys, os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from book_veto import MAX_LOSS, kept_moves  # noqa: E402

K = 4
MIN_SHARE = 0.10
M_CAP = 300
L_MIN, L_MAX = 0.05, 2.0
SCALE = 1000


def live_factor(st):
    """Elo-korrigierter, geglaetteter Live-Faktor (1 = neutral)."""
    if not st:
        return 1.0
    exp = st.get("exp", 0.5 * st["n"])
    p = 0.5 + (st["score"] - exp) / (st["n"] + K)
    return min(max(2 * p, L_MIN), L_MAX)


def apply_min_share(share):
    """Hebt Zuege unter MIN_SHARE an und skaliert den Rest herunter
    (iterativ, falls dabei weitere Zuege unter die Grenze rutschen)."""
    fixed = set()
    while True:
        low = {m for m in share if m not in fixed and share[m] < MIN_SHARE}
        if not low:
            return share
        fixed |= low
        rest = 1 - MIN_SHARE * len(fixed)
        free = sum(share[m] for m in share if m not in fixed)
        share = {m: (MIN_SHARE if m in fixed else share[m] / free * rest) for m in share}


def write_txt(variant, positions, path):
    """Schlichte Textausgabe fuer bookbuild: '<zugfolge> | <zug> <gewicht>'."""
    with open(path, "w") as f:
        f.write(f"# Martuni-Eigenbuch {variant}, erzeugt von tools/book_weights.py\n")
        f.write("# Format: <Zugfolge ab Grundstellung> | <Buchzug> <Gewicht>\n")
        for p in positions:
            seq = " ".join(p["moves_from_start"])
            for m in p["moves"]:
                f.write(f"{seq} | {m['uci']} {m['weight']}\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("variant", choices=MAX_LOSS)
    a = ap.parse_args()
    v = a.variant
    T = MAX_LOSS[v]

    analysis = {r["fen"]: r for r in json.load(open(f"book_work/{v}_analysis.json"))["results"]}
    veto = json.load(open(f"book_work/{v}_veto.json"))["results"]
    live = {p["fen"]: p for p in json.load(open(f"book_work/{v}_positions.json"))["positions"]}

    out = []
    for r in veto:
        kept = kept_moves(r, T)
        if len(kept) < 2:
            continue
        an = analysis[r["fen"]]
        played = live.get(r["fen"], {}).get("played", {})
        mscore = {m: an["candidates"][m]["score"] for m in kept}
        mbest = max(s for s in mscore.values() if s is not None)
        rows = {}
        for m in kept:
            sf_loss = r["verdict"][m]["sf_loss"]
            m_gap = min(mbest - mscore[m], M_CAP) if mscore[m] is not None else M_CAP
            q_sf = math.exp(-sf_loss / T)
            q_m = math.exp(-m_gap / T)
            L = live_factor(played.get(m))
            rows[m] = {"uci": m, "san": an["candidates"][m]["san"], "sf_loss": sf_loss,
                       "m_gap": m_gap, "q_sf": round(q_sf, 3), "q_m": round(q_m, 3),
                       "live": played.get(m), "L": round(L, 3), "raw": q_sf * q_m * L}
        total = sum(x["raw"] for x in rows.values())
        share = apply_min_share({m: x["raw"] / total for m, x in rows.items()})
        for m, x in rows.items():
            x["share"] = round(share[m], 4)
            x["weight"] = max(1, round(share[m] * SCALE))
            x["raw"] = round(x["raw"], 4)
        out.append({"fen": r["fen"], "ply": an["ply"], "moves_from_start": an["moves_from_start"],
                    "moves": sorted(rows.values(), key=lambda x: -x["weight"])})

    out.sort(key=lambda p: (p["ply"], p["fen"]))
    json.dump({"variant": v, "formula": {"T": T, "k": K, "min_share": MIN_SHARE, "m_cap": M_CAP,
                                         "L_range": [L_MIN, L_MAX], "scale": SCALE,
                                         "live": "elo-korrigiert"},
               "positions": out}, open(f"book_work/{v}_book.json", "w"), indent=1)
    write_txt(v, out, f"book_work/{v}_book.txt")
    n_moves = sum(len(p["moves"]) for p in out)
    missing = sum(1 for f in live if f not in analysis)
    print(f"{v}: {len(out)} Buchstellungen, {n_moves} Zuege -> book_work/{v}_book.json + .txt"
          f"  ({missing} neue Live-Stellungen noch ohne Analyse)")


if __name__ == "__main__":
    main()
