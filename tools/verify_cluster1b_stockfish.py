#!/usr/bin/env python3
"""Sanity-Check: bleibt Stockfishs Cluster-1b-Empfehlung bis Depth 30 stabil?

Wir lassen Stockfish jede Stellung bis `MAX_DEPTH` durchrechnen und protokollieren
den PV-Erstzug pro Iterationstiefe. Wenn die Empfehlung bei depth 17 nur eine
Momentaufnahme war, sieht man das hier — sie flippt dann irgendwo zwischen d18
und d30. Bleibt sie konstant, ist Stockfishs Urteil belastbar.
"""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path

STOCKFISH = "/usr/games/stockfish"
MAX_DEPTH = 30
THREADS = 2
HASH_MB = 512


@dataclass
class Probe:
    label: str
    fen: str
    engine_uci: str  # Martunis tatsaechliche Wahl (bad)
    engine_san: str
    sf_uci: str      # Stockfish-Best aus analyse-13.05.2026.json
    sf_san: str


PROBES: list[Probe] = [
    Probe("m1oQlfmG m18 W",
          "r2q1rk1/pp2n1b1/2p1Pnpp/3p4/1PP1p3/2N2B2/PB3PPP/R2QR1K1 w - - 0 18",
          "c3e4", "Nxe4", "c4d5", "cxd5"),
    Probe("54iwUiMx m25 B",
          "2rr2k1/1pqb1pp1/p1n1p2p/2P1P3/2PN1P2/4B1R1/P3Q1PP/2R3K1 b - - 7 25",
          "c6e7", "Ne7", "c6d4", "Nxd4"),
    Probe("W5AboGf0 m29 W",
          "5r1k/5nbp/3qQpp1/4p3/1r2P3/p5B1/P2N1PPP/R2R2K1 w - - 2 29",
          "e6g4", "Qg4", "e6d6", "Qxd6"),
]


def run_one(p: Probe) -> dict:
    proc = subprocess.Popen(
        [STOCKFISH],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, bufsize=1,
    )
    assert proc.stdin and proc.stdout

    def send(line: str) -> None:
        proc.stdin.write(line + "\n"); proc.stdin.flush()

    send("uci")
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "uciok":
            break
    send(f"setoption name Threads value {THREADS}")
    send(f"setoption name Hash value {HASH_MB}")
    send("isready")
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "readyok":
            break
    send("ucinewgame")
    send(f"position fen {p.fen}")
    send(f"go depth {MAX_DEPTH}")

    per_depth: dict[int, tuple[str, int | None]] = {}
    best: str | None = None
    while True:
        line = proc.stdout.readline()
        if not line:
            break
        line = line.rstrip()
        if line.startswith("info "):
            toks = line.split()
            # need: depth, score cp X (or mate X), pv ...
            try:
                d_idx = toks.index("depth")
                depth = int(toks[d_idx + 1])
            except (ValueError, IndexError):
                continue
            # skip selective-depth-only updates without pv
            if "pv" not in toks:
                continue
            # score
            score: int | None = None
            if "score" in toks:
                s_idx = toks.index("score")
                kind = toks[s_idx + 1]
                val = int(toks[s_idx + 2])
                if kind == "cp":
                    score = val
                elif kind == "mate":
                    # encode mate as +/-99000+plies
                    score = (99000 - abs(val)) * (1 if val > 0 else -1)
            pv_idx = toks.index("pv")
            if pv_idx + 1 < len(toks):
                per_depth[depth] = (toks[pv_idx + 1], score)
        elif line.startswith("bestmove"):
            parts = line.split()
            if len(parts) >= 2:
                best = parts[1]
            break

    send("quit")
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()
    return {"per_depth": per_depth, "best": best or ""}


def main() -> int:
    print(f"# Sanity-Check Stockfish, max_depth={MAX_DEPTH}, threads={THREADS}, hash={HASH_MB}MB\n")
    for p in PROBES:
        print(f"=== {p.label}  (Martuni: {p.engine_san}, SF-Best d17 war: {p.sf_san}) ===")
        r = run_one(p)
        per = r["per_depth"]
        final = r["best"]
        # show every 2nd depth from min to max for compactness
        ds = sorted(per)
        if not ds:
            print("  (keine info-Zeilen erhalten)\n")
            continue
        rows = []
        for d in ds:
            mv, sc = per[d]
            tag = "SF" if mv == p.sf_uci else ("ENG" if mv == p.engine_uci else "?")
            sc_str = f"{sc:+d}" if sc is not None and abs(sc) < 90000 else f"M{99000 - abs(sc):+d}" if sc else "?"
            rows.append(f"d{d:>2}:{tag}:{sc_str}({mv})")
        # print compactly
        print("  " + "  ".join(rows[:8]))
        if len(rows) > 8:
            print("  " + "  ".join(rows[8:16]))
        if len(rows) > 16:
            print("  " + "  ".join(rows[16:24]))
        if len(rows) > 24:
            print("  " + "  ".join(rows[24:]))
        # final assessment
        if final == p.sf_uci:
            verdict = f"STABIL → SF-Best ({p.sf_san})"
        elif final == p.engine_uci:
            verdict = f"FLIP zu Martuni-Wahl ({p.engine_san})"
        else:
            verdict = f"FLIP zu anderem Zug ({final})"
        print(f"  → final d{MAX_DEPTH}: {verdict}\n")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
