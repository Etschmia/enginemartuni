#!/usr/bin/env python3
"""Smoke-Test fuer Move-Ordering-Experimente an den Cluster-1b-Stellungen.

Vergleicht zwei Engine-Binaries (Baseline und Challenger) auf den 9
Cluster-1b-Stichproben aus `probe_missed_captures.py`. Pro Stellung
wird jedes Binary einmal aufgerufen; verglichen werden:

  - max erreichte Tiefe pro Stellung
  - finaler best move (UCI) — sowie Bewertung gegen probe-eigenes
    bad_uci / best_uci
  - Tiefe, ab der die Engine auf den besten Zug umschwenkt

Aufrufe (Standard 30 s pro Stellung × 9 × 2 = ~9 min):

    .venv/bin/python3 tools/probe_capture_ordering.py
    .venv/bin/python3 tools/probe_capture_ordering.py --movetime 60000

Binaries werden per Default auf
`target/release/martuni.backup-pre-mvv-20260516` (Baseline) und
`target/release/martuni-mvv-cp` (Challenger) gemappt. Beides via
`--baseline` / `--challenger` ueberschreibbar fuer kuenftige
Move-Ordering-Experimente.

Vorgeschichte: aus tools/probe_aspiration.py hervorgegangen
(Aspiration-Experiment 16.05.2026 verworfen, siehe
docs/aspiration-windows.md). Skript wurde generalisiert, weil derselbe
Probe-Aufbau fuer jeden Move-Ordering-Hebel passt.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_BASELINE = REPO / "target" / "release" / "martuni.backup-pre-mvv-20260516"
DEFAULT_CHALLENGER = REPO / "target" / "release" / "martuni-mvv-cp"

MAX_DEPTH = 16
DEFAULT_MOVETIME_MS = 30_000
HASH_MB = 256


@dataclass
class Probe:
    label: str
    fen: str
    bad_uci: str
    bad_san: str
    best_uci: str
    best_san: str


# Identisch zur Probe-Liste in tools/probe_missed_captures.py — Cluster-1b
# missed_capture-Stichprobe vom 15.05.2026.
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


@dataclass
class Result:
    max_depth: int
    per_depth: dict[int, str]   # depth -> first PV move
    best: str


_RE_INFO_DEPTH = re.compile(r"^info depth (\d+) .* pv (\S+)")


def probe_one(engine: Path, p: Probe, movetime_ms: int) -> Result:
    """Run engine on one position; return depth/PV-history and final move."""
    proc = subprocess.Popen(
        [str(engine)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
        bufsize=1,
    )
    assert proc.stdin and proc.stdout

    def send(line: str) -> None:
        assert proc.stdin
        proc.stdin.write(line + "\n")
        proc.stdin.flush()

    send("uci")
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
    send(f"go depth {MAX_DEPTH} movetime {movetime_ms}")

    per_depth: dict[int, str] = {}
    best: str = ""

    while True:
        line = proc.stdout.readline()
        if not line:
            break
        line = line.rstrip()
        if m := _RE_INFO_DEPTH.match(line):
            depth = int(m.group(1))
            pv_move = m.group(2)
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

    max_depth = max(per_depth) if per_depth else 0
    return Result(max_depth=max_depth, per_depth=per_depth, best=best)


def first_correct_depth(per_depth: dict[int, str], best_uci: str) -> int | None:
    """Kleinste Tiefe, ab der der best move durchgaengig bis zur Max-Tiefe gespielt wird."""
    if not per_depth or not best_uci:
        return None
    sorted_depths = sorted(per_depth)
    max_d = sorted_depths[-1]
    for d in sorted_depths:
        if per_depth[d] != best_uci:
            continue
        if all(per_depth.get(dd) == best_uci for dd in range(d, max_d + 1)):
            return d
    return None


def quality(p: Probe, best_uci: str) -> str:
    """G/B/?  — best-Zug, bad-Zug, oder weder noch."""
    if best_uci == p.best_uci:
        return "G"
    if best_uci == p.bad_uci:
        return "B"
    return "?"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--baseline", type=Path, default=DEFAULT_BASELINE,
                    help=f"Baseline-Binary (default: {DEFAULT_BASELINE.name})")
    ap.add_argument("--challenger", type=Path, default=DEFAULT_CHALLENGER,
                    help=f"Challenger-Binary (default: {DEFAULT_CHALLENGER.name})")
    ap.add_argument("--movetime", type=int, default=DEFAULT_MOVETIME_MS,
                    help=f"Movetime in ms (default: {DEFAULT_MOVETIME_MS})")
    args = ap.parse_args(argv)

    for label, path in [("Baseline", args.baseline), ("Challenger", args.challenger)]:
        if not path.exists():
            print(f"FEHLER: {label}-Binary nicht gefunden: {path}", file=sys.stderr)
            return 1

    print(f"# Move-Ordering Smoke-Test")
    print(f"#   Baseline   : {args.baseline.name}")
    print(f"#   Challenger : {args.challenger.name}")
    print(f"#   movetime   : {args.movetime} ms, max_depth {MAX_DEPTH}, hash {HASH_MB} MB")
    print(f"#   probes     : {len(PROBES)} Cluster-1b-Stellungen")
    print()
    hdr_a = "Stellung"
    hdr_b = "  Base maxD/best/flip→best/qual"
    hdr_c = "  Chal maxD/best/flip→best/qual"
    print(f"{hdr_a:22s} | {hdr_b} | {hdr_c}")
    print("-" * 120)

    counters = {"base": {"G": 0, "B": 0, "?": 0, "sum_d": 0},
                "chal": {"G": 0, "B": 0, "?": 0, "sum_d": 0}}
    transitions: list[tuple[str, str, str]] = []   # (label, base_qual, chal_qual)

    for p in PROBES:
        rb = probe_one(args.baseline, p, args.movetime)
        rc = probe_one(args.challenger, p, args.movetime)

        qb = quality(p, rb.best)
        qc = quality(p, rc.best)
        fb = first_correct_depth(rb.per_depth, p.best_uci)
        fc = first_correct_depth(rc.per_depth, p.best_uci)
        fb_s = f"d{fb}" if fb else "n/a"
        fc_s = f"d{fc}" if fc else "n/a"

        counters["base"][qb] += 1
        counters["chal"][qc] += 1
        counters["base"]["sum_d"] += rb.max_depth
        counters["chal"]["sum_d"] += rc.max_depth
        transitions.append((p.label, qb, qc))

        base_cell = f"{rb.max_depth:3d}/{rb.best:6s}/{fb_s:5s}/{qb}"
        chal_cell = f"{rc.max_depth:3d}/{rc.best:6s}/{fc_s:5s}/{qc}"
        print(f"{p.label:22s} | {base_cell:30s} | {chal_cell:30s}")

    print("-" * 120)
    n = len(PROBES)
    bc, cc = counters["base"], counters["chal"]
    print(f"Quality: Base  G={bc['G']} B={bc['B']} ?={bc['?']}    "
          f"Chal  G={cc['G']} B={cc['B']} ?={cc['?']}")
    print(f"Σ maxD : Base {bc['sum_d']} (Ø {bc['sum_d']/n:.2f})    "
          f"Chal {cc['sum_d']} (Ø {cc['sum_d']/n:.2f})    Δ {cc['sum_d']-bc['sum_d']:+d}")
    print()

    # Wertungs-Übergänge: wer hat sich verbessert/verschlechtert?
    improved = [t for t in transitions if t[1] != "G" and t[2] == "G"]
    regressed = [t for t in transitions if t[1] == "G" and t[2] != "G"]
    if improved:
        print("VERBESSERUNGEN (Chal G, Base nicht):")
        for label, qb, qc in improved:
            print(f"  + {label}  {qb} → {qc}")
    if regressed:
        print("REGRESSIONEN (Base G, Chal nicht):")
        for label, qb, qc in regressed:
            print(f"  - {label}  {qb} → {qc}")
    if not improved and not regressed:
        print("Keine Wertungs-Übergänge.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
