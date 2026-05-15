#!/usr/bin/env python3
"""Quiescence-/Baseline-Vergleich Martuni vs Stockfish fuer die Stellung
NACH `Qxd6 Nxd6` aus W5AboGf0. Material ist nach dem Tausch ausgeglichen.

Wir lassen beide Engines d1..d12 durchlaufen und tabellieren den Score
pro Tiefe. Falls Martunis Score schon bei d1 (im Wesentlichen statische
Eval + Quiescence) signifikant ueber Stockfishs Score liegt UND der
Abstand bis d10 nicht zusammenlaeuft, deutet das auf einen falschen
statischen Anker (Quiescence/Eval) hin.

Erwartung Stockfish (aus dem d30-Sanity-Check): ~ -32 cp aus White-Sicht
bei d30.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

MARTUNI = Path(__file__).resolve().parent.parent / "target" / "release" / "martuni"
STOCKFISH = "/usr/games/stockfish"
FEN = "5r1k/6bp/3n1pp1/4p3/1r2P3/p5B1/P2N1PPP/R2R2K1 w - - 0 30"


def run_engine(engine: str, depth_max: int, movetime_ms: int) -> list[tuple[int, int | None, str | None]]:
    proc = subprocess.Popen(
        [engine],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, bufsize=1,
    )
    assert proc.stdin and proc.stdout
    def send(line: str) -> None:
        proc.stdin.write(line + "\n"); proc.stdin.flush()
    send("uci")
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "uciok": break
    send("setoption name Hash value 256")
    if "stockfish" in engine.lower():
        send("setoption name Threads value 1")  # vergleichbarer Single-Thread
    send("isready")
    while True:
        line = proc.stdout.readline()
        if not line or line.strip() == "readyok": break
    send("ucinewgame")
    send(f"position fen {FEN}")
    send(f"go depth {depth_max} movetime {movetime_ms}")

    rows: list[tuple[int, int | None, str | None]] = []
    while True:
        line = proc.stdout.readline()
        if not line: break
        line = line.rstrip()
        if line.startswith("info "):
            toks = line.split()
            try:
                d = int(toks[toks.index("depth") + 1])
            except (ValueError, IndexError):
                continue
            sc = None
            if "score" in toks:
                s = toks.index("score")
                if toks[s+1] == "cp":
                    try: sc = int(toks[s+2])
                    except: pass
                elif toks[s+1] == "mate":
                    try:
                        v = int(toks[s+2])
                        sc = (99000 - abs(v)) * (1 if v > 0 else -1)
                    except: pass
            pv = None
            if "pv" in toks:
                i = toks.index("pv")
                if i + 1 < len(toks): pv = toks[i+1]
            rows.append((d, sc, pv))
        elif line.startswith("bestmove"):
            break
    send("quit")
    try: proc.wait(timeout=2)
    except subprocess.TimeoutExpired: proc.kill()
    return rows


def latest_per_depth(rows: list[tuple[int, int | None, str | None]]) -> dict[int, tuple[int | None, str | None]]:
    """Falls Stockfish mehrere info-Zeilen pro depth emittiert (Multi-PV / re-search),
    nimm die jeweils letzte."""
    out: dict[int, tuple[int | None, str | None]] = {}
    for d, s, m in rows:
        out[d] = (s, m)
    return out


def main() -> int:
    print(f"# Quiescence-/Baseline-Trace nach Qxd6 Nxd6")
    print(f"# FEN: {FEN}")
    print(f"# Erwartung (SF d30): ~ -32 cp White-Sicht\n")

    print("Martuni laeuft ...")
    mart = latest_per_depth(run_engine(str(MARTUNI), depth_max=12, movetime_ms=30_000))
    print("Stockfish laeuft ...")
    sf = latest_per_depth(run_engine(STOCKFISH, depth_max=12, movetime_ms=30_000))

    print()
    print(f"{'Tiefe':>5}  {'Martuni':>9}  {'Stockfish':>9}  {'Diff':>6}  {'M-PV':>6}  {'SF-PV':>6}")
    print("-" * 60)
    all_depths = sorted(set(mart) | set(sf))
    for d in all_depths:
        ms, mm = mart.get(d, (None, None))
        ss, sm = sf.get(d, (None, None))
        ms_s = "?" if ms is None else format(ms, "+5d")
        ss_s = "?" if ss is None else format(ss, "+5d")
        diff_s = "?" if (ms is None or ss is None) else format(ms - ss, "+5d")
        print(f"{d:>5}  {ms_s:>9}  {ss_s:>9}  {diff_s:>6}  {mm or '?':>6}  {sm or '?':>6}")
    print()
    # Endbefund
    if mart and sf:
        last_d = max(set(mart) & set(sf))
        diff = (mart[last_d][0] or 0) - (sf[last_d][0] or 0)
        print(f"Bei d{last_d} liegt Martuni {diff:+d} cp ueber Stockfish.")
        if abs(diff) > 50:
            print("  -> persistenter Eval-Offset, Quiescence/statische Eval verdaechtig.")
        else:
            print("  -> Abstand klein; Verdacht eher Such-Pfad/Move-Ordering an der Wurzel.")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
