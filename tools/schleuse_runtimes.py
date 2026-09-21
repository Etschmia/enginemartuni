#!/usr/bin/env python3
"""Per-game analysis runtimes from the schleuse worker log, grouped by engine.

Usage: tools/schleuse_runtimes.py [logfile]
Default logfile: logs/schleuse_worker.log relative to the repo root.
"""

from __future__ import annotations

import datetime
import pathlib
import re
import statistics
import sys

TS_RE = "%Y-%m-%dT%H:%M:%S.%fZ"
SELECTED_RE = re.compile(r"Engine selected: (\S+) for '(.+)'")


def parse(path: pathlib.Path) -> list[tuple[str, float, str]]:
    rows: list[tuple[str, float, str]] = []
    start: datetime.datetime | None = None
    engine = name = ""
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            stamp = datetime.datetime.strptime(line.split()[0], TS_RE)
        except (ValueError, IndexError):
            continue
        match = SELECTED_RE.search(line)
        if match:
            start, engine, name = stamp, match.group(1), match.group(2)
        elif "OK uploaded" in line and start is not None:
            rows.append((engine, (stamp - start).total_seconds(), name))
            start = None
    return rows


def main() -> int:
    default = pathlib.Path(__file__).resolve().parent.parent / "logs" / "schleuse_worker.log"
    path = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else default
    if not path.exists():
        print(f"no such log: {path}", file=sys.stderr)
        return 1
    rows = parse(path)
    if not rows:
        print("no completed analyses in log")
        return 0
    for engine, secs, name in rows:
        print(f"{engine:16} {secs:6.0f}s  {name[:50]}")
    print()
    for engine in sorted({r[0] for r in rows}):
        vals = [r[1] for r in rows if r[0] == engine]
        print(
            f"{engine:16} n={len(vals):3}  median {statistics.median(vals):5.0f}s  "
            f"range {min(vals):4.0f}-{max(vals):4.0f}s  total {sum(vals) / 60:6.1f}min"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
