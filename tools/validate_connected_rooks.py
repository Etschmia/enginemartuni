#!/usr/bin/env python3
"""Validierung des connected_rooks_pair-Bonus (aktuell 150 cp) gegen
mehrere Stellungstypen. Pro Stellung:

  1. Martunis Eval-Breakdown via UCI-Kommando `eval`
  2. Stockfish-Statik (d1 als bestmoeglicher Anker fuer statische Eval)
  3. Differenz aufzeigen, mit besonderem Augenmerk auf den
     connected_rooks-Beitrag.

Sample umfasst:
  - die zweite echte Cluster-1b-Stellung (54iwUiMx m25 B nach Ne7)
  - zwei "klassische" Stellungen, in denen verbundene Tuerme echt
    Wert haben (doppelt auf offener Linie, 7. Reihe mit Backup)
  - eine typische Mittelspiel-Roehre (beide Seiten verbundene Tuerme)
  - eine Stellung, in der der Bonus offensichtlich ueberbewertet (passive
    Tuerme auf Grundreihe ohne Plan, wie die W5AboGf0-Folgestellung).
"""

from __future__ import annotations

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

# Wichtig bei Output-Umleitung via Pipe/Datei: print() wuerde sonst
# pufferweise rausgeben und bei Haengern gar nichts liefern.
sys.stdout.reconfigure(line_buffering=True)  # type: ignore[attr-defined]

MARTUNI = str(Path(__file__).resolve().parent.parent / "target" / "release" / "martuni")
STOCKFISH = "/usr/games/stockfish"


@dataclass
class Probe:
    label: str
    fen: str
    note: str


PROBES: list[Probe] = [
    Probe(
        "W5AboGf0 post Qxd6 Nxd6 (Referenz)",
        "5r1k/6bp/3n1pp1/4p3/1r2P3/p5B1/P2N1PPP/R2R2K1 w - - 0 30",
        "Cluster-1b Referenzfall: Martuni +121, Stockfish ~-40, connected_rooks +150",
    ),
    Probe(
        "54iwUiMx m25 B vor Ne7",
        "2rr2k1/1pqb1pp1/p1n1p2p/2P1P3/2PN1P2/4B1R1/P3Q1PP/2R3K1 b - - 7 25",
        "Cluster-1b Stellung 2 — Stockfish d30 fuer Nxd4 stabil",
    ),
    Probe(
        "Doppelt auf offener c-Linie (klassisch)",
        "4k3/pp3ppp/8/8/8/8/PP3PPP/2RR2K1 w - - 0 1",
        "Beide Tuerme verbunden, c-Linie ist offen — hier sollte der Bonus zaehlen",
    ),
    Probe(
        "Standard-Mittelspiel beide Rochaden",
        "r2qr1k1/pp3ppp/2n1pn2/2bp4/3P4/2N1PN2/PP1QBPPP/R3R1K1 w - - 0 1",
        "Beide Seiten haben verbundene Tuerme — klassische Eroeffnungsstruktur",
    ),
    Probe(
        "Turm-Endspiel mit aktiver Position",
        "4r1k1/pp3ppp/8/8/4R3/8/PP3PPP/4R1K1 w - - 0 1",
        "Weiss verdoppelt auf e-Linie, Schwarz hat einen Turm — Bonus sollte zaehlen",
    ),
]


def run_martuni_eval(fen: str) -> tuple[int | None, dict[str, tuple[int, int]]]:
    """Schickt position+eval, liest bis 'readyok' nach 'isready'. Robust auch
    bei Pipe-Ausgabe (kein anhaengender Subprozess wenn read haengt)."""
    proc = subprocess.Popen(
        [MARTUNI],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, bufsize=1,
    )
    assert proc.stdin and proc.stdout

    try:
        # Alle Kommandos auf einmal — dann darf der reader bis quit lesen.
        proc.stdin.write(
            "uci\n"
            "isready\n"
            f"position fen {fen}\n"
            "eval\n"
            "isready\n"
            "quit\n"
        )
        proc.stdin.flush()

        # Drain bis EOF — engine schliesst nach quit. Wir filtern relevante Zeilen.
        total: int | None = None
        components: dict[str, tuple[int, int]] = {}
        readyok_count = 0
        for line in proc.stdout:  # iter bis EOF
            line = line.rstrip()
            if line == "readyok":
                readyok_count += 1
            if line.startswith("info string total"):
                m = re.search(r"total\s+(-?\d+)", line)
                if m: total = int(m.group(1))
            elif line.startswith("info string"):
                m = re.match(r"info string\s+([a-z_]+)\s+W=\s*(-?\d+)\s+B=\s*(-?\d+)", line)
                if m:
                    components[m.group(1)] = (int(m.group(2)), int(m.group(3)))
        proc.wait(timeout=5)
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=2)
    return total, components


def run_stockfish_static(fen: str, depth: int = 1, movetime_ms: int = 2000) -> int | None:
    """Stockfish empfaengt quit nicht im selben Batch wie go — sonst stoppt
    er die Suche bevor info-Zeilen rausgehen. Wir schicken quit erst nach
    bestmove."""
    proc = subprocess.Popen(
        [STOCKFISH],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        text=True, bufsize=1,
    )
    assert proc.stdin and proc.stdout
    score: int | None = None
    try:
        proc.stdin.write(
            "uci\n"
            "setoption name Threads value 1\n"
            "isready\n"
            f"position fen {fen}\n"
            f"go depth {depth} movetime {movetime_ms}\n"
        )
        proc.stdin.flush()
        # Lies bis bestmove — dann ist die Suche vollstaendig durchgelaufen
        # und alle info-Zeilen sind raus.
        while True:
            line = proc.stdout.readline()
            if not line: break
            line = line.rstrip()
            if line.startswith("info ") and "score cp" in line:
                m = re.search(r"score cp (-?\d+)", line)
                if m: score = int(m.group(1))
            if line.startswith("bestmove"):
                break
        proc.stdin.write("quit\n"); proc.stdin.flush()
        proc.wait(timeout=5)
    finally:
        if proc.poll() is None:
            proc.kill(); proc.wait(timeout=2)
    # Stockfish-Score steht aus Sicht des Ziehenden — wir wollen die Weiss-Sicht.
    parts = fen.split()
    if len(parts) >= 2 and parts[1] == "b" and score is not None:
        score = -score
    return score


def main() -> int:
    print(f"# Validierung connected_rooks_pair (aktuell 150 cp)\n")
    rows = []
    for p in PROBES:
        print(f"=== {p.label} ===")
        print(f"  FEN: {p.fen}")
        print(f"  ({p.note})")
        m_total, m_comp = run_martuni_eval(p.fen)
        s_total = run_stockfish_static(p.fen, depth=1, movetime_ms=2000)
        s_total_d6 = run_stockfish_static(p.fen, depth=6, movetime_ms=5000)
        cr_w, cr_b = m_comp.get("connected_rooks", (0, 0))
        diff_cr = cr_w - cr_b
        if m_total is None or s_total is None:
            print("  (Fehler beim Lesen der Eval)"); continue
        # Wenn Schwarz am Zug ist, ist Martunis total bereits aus Weiss-Sicht — keine Drehung.
        gap = m_total - s_total
        gap_d6 = (m_total - s_total_d6) if s_total_d6 is not None else None
        ohne_cr = m_total - diff_cr
        rows.append((p.label, m_total, s_total, s_total_d6, gap, gap_d6, diff_cr, ohne_cr))
        print(f"  Martuni total:           {m_total:+5d} cp")
        print(f"  Stockfish d1:            {s_total:+5d} cp")
        if s_total_d6 is not None:
            print(f"  Stockfish d6:            {s_total_d6:+5d} cp")
        print(f"  Diff (Mart - SF d1):     {gap:+5d} cp")
        print(f"  connected_rooks Diff:    {diff_cr:+5d} cp  (W={cr_w}, B={cr_b})")
        print(f"  Mart OHNE conn_rooks:    {ohne_cr:+5d} cp  -> Diff zu SF d1: {ohne_cr - s_total:+d} cp")
        print()

    print("\n## Zusammenfassung")
    print(f"{'Stellung':<45} {'Mart':>5} {'SFd1':>5} {'SFd6':>5} {'Gap':>5} {'ConnR':>6} {'ohneCR':>7} {'ResidGap':>9}")
    for label, mt, s1, s6, g, g6, cr, oc in rows:
        s6s = f"{s6:+5d}" if s6 is not None else "  ?  "
        rg = oc - s1
        print(f"{label:<45} {mt:>+5d} {s1:>+5d} {s6s:>5} {g:>+5d} {cr:>+6d} {oc:>+7d} {rg:>+9d}")
    print()
    print("Lesehilfe:")
    print("  Gap     = aktueller Martuni-Bias gegen Stockfish d1 (statisch)")
    print("  ConnR   = wieviel der Bias allein vom connected_rooks_pair-Term kommt")
    print("  ohneCR  = was Martuni saehe, wenn der Term 0 waere")
    print("  ResidGap= Rest-Bias nach Abzug des connected_rooks-Beitrags")
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
