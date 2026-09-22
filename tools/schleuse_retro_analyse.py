#!/usr/bin/env python3
"""One-off: analyse the games quarantined on the Martuni server.

98 PGNs were quarantined between 2026-09-14 and 2026-09-20 after failing three
times on the previous analysis host (all tracebacks point at /workspace/...).
None of them ever failed here. This re-runs them and delivers the results
through the normal channel.

Deliberate design choices:

* Writes only to `inbox/`, which the protocol assigns to the analysis host.
  The quarantine file belongs to the Martuni server and is never touched —
  `schleuse_sync.py` clears an entry by itself on successful import
  (`failures.pop`), so a successful re-run cleans up after itself.
* Reads the PGNs straight from the server's `game_records/`, read-only. They
  are not in the outbox (quarantined games are excluded from export), and the
  outbox is not ours to write.
* Logs the **full** error text locally on failure. The server truncates it to
  300 characters (`schleuse_sync.py:186`), which reliably cuts a Python
  traceback before the exception line — that is exactly why the cause of the
  89 crashes is unknown today. Whatever fails here will be explainable.
* Runs one game at a time and skips anything that already has an inbox result,
  so it can be interrupted and restarted without losing work.
"""

from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import schleuse_worker as w  # noqa: E402  (path set above)

REPO_ROOT = Path(__file__).resolve().parent.parent
RETRO_LOG = Path(
    os.environ.get("SCHLEUSE_RETRO_LOG", str(REPO_ROOT / "logs" / "schleuse_retro.log"))
)
REMOTE_GAME_DIR = os.environ.get(
    "SCHLEUSE_REMOTE_GAME_DIR", "/home/librechat/lichess-bot/game_records"
)
QUARANTINE = os.environ.get(
    "SCHLEUSE_REMOTE_QUARANTINE",
    "/home/librechat/enginemartuni/tools/analyze_cron.quarantine.json",
)


def log(msg: str) -> None:
    line = f"{w.utc_now_iso()} {msg}"
    RETRO_LOG.parent.mkdir(parents=True, exist_ok=True)
    with RETRO_LOG.open("a", encoding="utf-8") as fh:
        fh.write(line + "\n")
    print(line, flush=True)


def quarantined_names() -> list[str]:
    raw = w.ssh(f"cat {shlex.quote(QUARANTINE)}", check=True).stdout
    return sorted(json.loads(raw).get("failures", {}))


def already_done() -> set[str]:
    return w.list_inbox_done_names()


def main() -> int:
    names = quarantined_names()
    done = already_done()
    todo = [n for n in names if n not in done]
    log(f"Quarantined: {len(names)}; already in inbox: {len(names) - len(todo)}; to do: {len(todo)}")

    work = w.WORK_DIR / "retro"
    work.mkdir(parents=True, exist_ok=True)

    ok = failed = missing = 0
    for i, name in enumerate(todo, 1):
        local_pgn = work / name
        local_json = work / f"{name}.json"
        for p in (local_pgn, local_json):
            p.unlink(missing_ok=True)

        try:
            w.scp_from(f"{REMOTE_GAME_DIR}/{name}", local_pgn)
        except Exception as exc:
            log(f"[{i}/{len(todo)}] MISSING {name!r}: {exc}")
            missing += 1
            continue

        pgn_text = local_pgn.read_text(encoding="utf-8", errors="replace")
        engine = w.detect_engine(pgn_text)
        env = os.environ.copy()
        env["PATH"] = f"{w.ENGINE_PATH}:{env.get('PATH', '')}"
        cmd = [
            w.PYTHON, w.ANALYZE_SCRIPT,
            "--engine", engine,
            "--depth", str(w.DEPTH),
            "--threads", str(w.THREADS),
            "--hash", str(w.HASH),
            "--min-movetime", str(w.MIN_MOVETIME),
            "--output", str(local_json),
            str(local_pgn),
        ]
        try:
            proc = subprocess.run(
                cmd, capture_output=True, text=True, env=env,
                cwd=str(work), timeout=w.ANALYZE_TIMEOUT_S,
            )
        except subprocess.TimeoutExpired:
            log(f"[{i}/{len(todo)}] TIMEOUT {name!r} engine={engine} "
                f"after {w.ANALYZE_TIMEOUT_S}s")
            w.write_failed_and_upload(
                name, f"analyze timed out after {w.ANALYZE_TIMEOUT_S}s (engine={engine})",
                None, engine,
            )
            failed += 1
            continue

        if proc.returncode != 0 or not local_json.exists():
            err = (proc.stderr or proc.stdout or f"rc={proc.returncode}").strip()
            # FULL text here; the server would keep only the first 300 chars.
            log(f"[{i}/{len(todo)}] FAIL {name!r} engine={engine} rc={proc.returncode}\n"
                f"--- full error ---\n{err}\n--- end ---")
            w.write_failed_and_upload(name, err[:2000], proc.returncode, engine)
            failed += 1
            continue

        w.tag_variant(local_json, w.pgn_variant(pgn_text))
        w.upload_inbox(local_json, f"{name}.json")
        n_bl = len(json.loads(local_json.read_text(encoding="utf-8")).get("blunders", []))
        log(f"[{i}/{len(todo)}] OK {name!r} engine={engine} blunders={n_bl}")
        ok += 1

    log(f"DONE: ok={ok} failed={failed} missing={missing} of {len(todo)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
