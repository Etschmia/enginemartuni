#!/usr/bin/env python3
"""Stufe-1-Test-Stellungen fuer den Pawn-Endgame-Guard.

Fuehrt UCI `eval` ueber Martuni auf jeder Stellung aus, parst den
Breakdown und prueft:

  1. dass der pawn_endgame_guard-Term in den dafuer vorgesehenen
     Stellungen aktiv ist (|w-b| > 0)
  2. dass er in der NPM-Gate-Boundary inaktiv ist (w == b == 0)

Ausgabe pro Stellung: einzeilig (PASS/FAIL) plus optional Breakdown-Snippet
mit `--verbose`.

Aufruf:
    .venv/bin/python3 tools/probe_pawn_endgame_guard.py
    .venv/bin/python3 tools/probe_pawn_endgame_guard.py --verbose
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ENGINE = REPO_ROOT / "target" / "release" / "martuni"


@dataclass
class Probe:
    label: str
    fen: str
    expectation: str
    # expectation values:
    #   "guard_positive_for_white" → Diff > 0
    #   "guard_negative_for_white" → Diff < 0  (= Bonus fuer Schwarz, z.B. Rook-Pawn-Strafe)
    #   "guard_inactive"           → Diff == 0
    #   "engine_picks"             → Engine soll auf gegebener Tiefe einen
    #                                 der best_moves spielen
    best_moves: list[str] | None = None  # nur fuer expectation="engine_picks"
    depth: int = 10
    note: str = ""


PROBES: list[Probe] = [
    # --- A. stickshark99-Engine-Verifikation -------------------------------
    # Hier wird NICHT der Term direkt geprueft (in den Ausgangs-FENs steht
    # der Koenig oft noch nicht auf einem Schluesselfeld), sondern das
    # Engine-Verhalten: bei aktiver Guard-Logik sollte die Engine den
    # richtigen Zug finden, der in der Original-Partie verloren ging.
    Probe(
        label="stickshark99 MH3BeAfV ply 91 → best Kc4",
        fen="3k4/5p2/1N1P4/p1PK2p1/1n4P1/5P1p/7P/8 w - - 1 46",
        expectation="engine_picks",
        best_moves=["d5c4"],
        depth=12,
        note="Engine spielte Kd4 (loss 154cp). Best Kc4. Term soll im "
        "Such-Subtree die richtige Koenig-Richtung favorisieren.",
    ),
    Probe(
        label="stickshark99 MH3BeAfV ply 105 → best Ka6",
        fen="4k3/5p2/1N1P4/1KP3p1/3n2P1/5P1p/7P/8 w - - 7 53",
        expectation="engine_picks",
        best_moves=["b5a6"],
        depth=12,
        note="Engine spielte Kb4 (loss 258cp). Best Ka6.",
    ),
    Probe(
        label="stickshark99 HxAG2UV8 ply 129 → best Ke4",
        fen="8/6p1/1Pk1n2p/p3P2P/8/1p2K3/1B6/8 w - - 4 65",
        expectation="engine_picks",
        best_moves=["e3e4"],
        depth=12,
        note="Engine spielte Kd3 (loss 381cp). Best Ke4.",
    ),

    # --- B. Lehrbuch-Term-Aktivierung --------------------------------------
    Probe(
        label="Lehrbuch: direkte Opposition (Schwarz am Zug)",
        fen="4k3/8/4K3/8/8/8/8/8 b - - 0 1",
        expectation="guard_positive_for_white",
        note="Ke6 vs Ke8, 1 Feld dazwischen, Schwarz am Zug → Weiss hat "
        "Opposition, Bonus +12 cp.",
    ),
    Probe(
        label="Lehrbuch: Schluesselfeld d6 vor e5",
        fen="4k3/8/3K4/4P3/8/8/8/8 b - - 0 1",
        expectation="guard_positive_for_white",
        note="Bauer e5 (Rang 5) + Koenig d6 (Schluesselfeld) → Bonus "
        "key_square_bonus_by_rank[4] = 28 cp.",
    ),
    Probe(
        label="Lehrbuch: Rook-Pawn-Remis h5 + Kh8",
        fen="7k/8/6K1/7P/8/8/8/8 b - - 0 1",
        expectation="guard_negative_for_white",
        note="h-Bauer + schwarzer K in Promo-Ecke. Term soll den weissen "
        "Passbauer-Optimismus daempfen: Strafe -60 cp.",
    ),

    # --- C. NPM-Gate-Boundary ----------------------------------------------
    Probe(
        label="Boundary: Mittelspiel mit Tuermen (NPM 1000)",
        fen="r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1",
        expectation="guard_inactive",
        note="Je Seite NPM 1000 > Gate 700.",
    ),
    Probe(
        label="Boundary: Mittelspiel mit Damen (NPM 900)",
        fen="3qk3/pppppppp/8/8/8/8/PPPPPPPP/3QK3 w - - 0 1",
        expectation="guard_inactive",
        note="Je Seite NPM 900 > Gate 700.",
    ),
]


def run_eval(fen: str) -> dict[str, str]:
    """Startet Martuni, fuettert `position fen ... / eval / quit`, parst
    den Breakdown in ein Dict line-name → Wert-Text."""
    if not ENGINE.exists():
        sys.exit(
            f"FEHLER: Binary {ENGINE} fehlt — cargo build --release zuerst."
        )
    cmd = [str(ENGINE)]
    stdin = f"uci\nisready\nposition fen {fen}\neval\nquit\n"
    res = subprocess.run(
        cmd, input=stdin, capture_output=True, text=True, timeout=15,
    )
    breakdown: dict[str, str] = {}
    for line in res.stdout.splitlines():
        if not line.startswith("info string "):
            continue
        rest = line[len("info string "):]
        if "  " in rest:
            # Form: "  name  W=...  B=...  Diff=..."
            tokens = rest.split()
            if len(tokens) >= 1:
                breakdown[tokens[0]] = rest
    return breakdown


def run_bestmove(fen: str, depth: int) -> str:
    """Laesst die Engine eine Stellung bis `depth` durchsuchen und gibt
    den UCI-Bestmove zurueck."""
    cmd = [str(ENGINE)]
    stdin = (
        "uci\nisready\n"
        f"position fen {fen}\n"
        f"go depth {depth}\n"
        "quit\n"
    )
    res = subprocess.run(
        cmd, input=stdin, capture_output=True, text=True, timeout=60,
    )
    for line in res.stdout.splitlines():
        if line.startswith("bestmove "):
            return line.split()[1]
    return "?"


_DIFF_RE = re.compile(r"Diff=\s*(-?\d+)")


def field_diff(line: str) -> int:
    """Extrahiert den Diff-Wert aus einer Breakdown-Zeile. Format:
    'info string  ... W=    28  B=     0  Diff=    28' — Felder sind
    rechtsbuendig auf 6 Zeichen formatiert, daher Regex statt simple
    split-by-whitespace."""
    m = _DIFF_RE.search(line)
    return int(m.group(1)) if m else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()

    passes = 0
    fails = 0
    for probe in PROBES:
        if probe.expectation == "engine_picks":
            mv = run_bestmove(probe.fen, probe.depth)
            ok = mv in (probe.best_moves or [])
            tag = "PASS" if ok else "FAIL"
            if ok:
                passes += 1
            else:
                fails += 1
            print(
                f"[{tag}] {probe.label}  (depth={probe.depth}, played={mv}, "
                f"expected={'|'.join(probe.best_moves or [])})"
            )
            if args.verbose or not ok:
                print(f"        fen: {probe.fen}")
                if probe.note:
                    print(f"        note: {probe.note}")
            continue

        bd = run_eval(probe.fen)
        line = bd.get("pawn_endgame_guard", "")
        diff = field_diff(line) if line else 0

        if probe.expectation == "guard_positive_for_white":
            ok = diff > 0
        elif probe.expectation == "guard_negative_for_white":
            ok = diff < 0
        elif probe.expectation == "guard_inactive":
            ok = diff == 0
        else:
            ok = False

        tag = "PASS" if ok else "FAIL"
        if ok:
            passes += 1
        else:
            fails += 1
        print(f"[{tag}] {probe.label}  (diff={diff}cp, expected={probe.expectation})")
        if args.verbose or not ok:
            print(f"        fen: {probe.fen}")
            if probe.note:
                print(f"        note: {probe.note}")
            if line:
                print(f"        line: {line}")
            else:
                print(f"        (kein pawn_endgame_guard-Eintrag im Breakdown)")
    print(f"\n{passes} passes / {fails} fails / {len(PROBES)} total")
    return 0 if fails == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
