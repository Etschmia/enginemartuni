#!/usr/bin/env python3
"""Cluster-1-Stichprobe: in welcher Iterationstiefe schwenkt Martuni von der
falschen auf die richtige Capture um?

Pro Test-Stellung wird `go depth <N>` an die Engine geschickt; aus den
`info depth K ... pv <move> ...` Zeilen liest das Skript den jeweils besten
ersten PV-Zug pro abgeschlossener Iteration. Reportet wird die kleinste Tiefe
K, ab der die Engine konsistent (bis Maximaltiefe) auf den richtigen Zug
umschwenkt -- oder "n/a", wenn sie den falschen Zug bis zum Schluss hält.

Aufruf: .venv/bin/python3 tools/probe_missed_captures.py
"""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ENGINE = Path(__file__).resolve().parent.parent / "target" / "release" / "martuni"
MAX_DEPTH = 14
MOVETIME_MS = 30_000  # Hard-Cap pro Stellung; `go depth` allein deckelt die Suche
                      # nicht (calculate_think_time fallback ~1s), darum beides
                      # setzen und auf depth-Stopp hoffen.
HASH_MB = 256


@dataclass
class Probe:
    label: str
    fen: str
    bad_uci: str
    bad_san: str
    best_uci: str
    best_san: str


PROBES: list[Probe] = [
    Probe("H62xk9vz m14 W",
          "r2q1rk1/pp1b1ppp/2n1p3/3p4/3P4/3BPN2/PPQn1PPP/R3K2R w KQ - 0 14",
          "e1d2", "Kxd2", "c2d2", "Qxd2"),
    Probe("zcBr22eo m24 B",
          "r3r1k1/p1B2pp1/Bp3n1p/3b4/2PR4/n4P2/5KPP/2N4R b - - 0 24",
          "a3c4", "Nxc4", "d5c4", "Bxc4"),
    Probe("wo4G1Ae5 m19 W",
          "r4r1k/p1p3pp/2pbN3/4p3/2Q1p1bq/2P2P2/PP4PP/R4RK1 w - - 0 19",
          "c4e4", "Qxe4", "f3g4", "fxg4"),
    Probe("m1oQlfmG m18 W",
          "r2q1rk1/pp2n1b1/2p1Pnpp/3p4/1PP1p3/2N2B2/PB3PPP/R2QR1K1 w - - 0 18",
          "c3e4", "Nxe4", "c4d5", "cxd5"),
    Probe("54iwUiMx m25 B",
          "2rr2k1/1pqb1pp1/p1n1p2p/2P1P3/2PN1P2/4B1R1/P3Q1PP/2R3K1 b - - 7 25",
          "c6e7", "Ne7", "c6d4", "Nxd4"),
    Probe("25eZUsMT m19 W",
          "r3k2r/5p1p/2p2p2/1p1p4/p1nPbN1N/bP2P1PB/P4P1P/R1R3K1 w kq - 1 19",
          "a1e1", "Re1", "b3c4", "bxc4"),
    Probe("W5AboGf0 m29 W",
          "5r1k/5nbp/3qQpp1/4p3/1r2P3/p5B1/P2N1PPP/R2R2K1 w - - 2 29",
          "e6g4", "Qg4", "e6d6", "Qxd6"),
    Probe("FLSJc0Sm m12 B",
          "r1b4r/pp2kppp/2n1p3/4P3/1b2N3/2N5/PPP2PPP/R1B1K2R b KQ - 0 12",
          "b4c3", "Bxc3+", "c6e5", "Nxe5"),
    Probe("wo4G1Ae5 m15 W",
          "r4rk1/p1p3pp/2pb2q1/4pp2/4P1bB/2PQ1N2/PP3PPP/R4RK1 w - - 0 15",
          "f3g5", "Ng5", "e4f5", "exf5"),
]


def probe_one(p: Probe) -> dict:
    """Run engine on one position; return {depth -> bestmove_uci} plus final."""
    cmd = [str(ENGINE)]
    proc = subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        bufsize=1,
    )
    assert proc.stdin and proc.stdout

    def send(line: str) -> None:
        proc.stdin.write(line + "\n")
        proc.stdin.flush()

    send("uci")
    # Drain until uciok
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "uciok":
            break
    send(f"setoption name Hash value {HASH_MB}")
    send("isready")
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "readyok":
            break

    send("ucinewgame")
    send(f"position fen {p.fen}")
    send(f"go depth {MAX_DEPTH} movetime {MOVETIME_MS}")

    per_depth: dict[int, str] = {}
    best: str | None = None
    while True:
        line = proc.stdout.readline()
        if not line:
            break
        line = line.rstrip()
        if line.startswith("info "):
            # find depth and pv
            toks = line.split()
            try:
                d_idx = toks.index("depth")
                depth = int(toks[d_idx + 1])
            except (ValueError, IndexError):
                continue
            if "pv" in toks:
                pv_idx = toks.index("pv")
                if pv_idx + 1 < len(toks):
                    pv_move = toks[pv_idx + 1]
                    per_depth[depth] = pv_move
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
    if not ENGINE.exists():
        print(f"FEHLER: engine binary nicht gefunden: {ENGINE}", file=sys.stderr)
        return 1

    print(f"# Stichprobe Cluster-1 missed_capture, max_depth={MAX_DEPTH}, "
          f"engine={ENGINE.name}\n")
    print(f"{'Stellung':22s} {'good?':5s} {'depth->correct':18s} "
          f"{'final_bm':10s}  Pfad (Zug pro Tiefe)")
    print("-" * 110)

    for p in PROBES:
        r = probe_one(p)
        per_depth = r["per_depth"]
        final = r["best"]

        # Mark per-depth as 'g' (good=best), 'b' (bad=engine-played), '.' (other)
        marks: list[str] = []
        first_g: int | None = None
        sticks: bool = True
        for d in sorted(per_depth):
            m = per_depth[d]
            if m == p.best_uci:
                marks.append(f"{d}:g")
                if first_g is None:
                    first_g = d
            elif m == p.bad_uci:
                marks.append(f"{d}:b")
                sticks = sticks and (first_g is None)
            else:
                marks.append(f"{d}:?({m})")

        # find depth at which engine permanently flips to best
        flip_depth: int | None = None
        if first_g is not None:
            # check it sticks from there to MAX_DEPTH
            stuck = all(per_depth.get(d) == p.best_uci
                        for d in range(first_g, max(per_depth) + 1))
            if stuck:
                flip_depth = first_g

        good_now = "G" if final == p.best_uci else ("B" if final == p.bad_uci else "?")
        depth_str = (f"≥{flip_depth}" if flip_depth is not None else
                     ("kurz " + str(first_g) if first_g else "n/a"))
        path_str = " ".join(marks)
        print(f"{p.label:22s} {good_now:5s} {depth_str:18s} {final:10s}  {path_str}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
