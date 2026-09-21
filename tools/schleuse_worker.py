#!/usr/bin/env python3
"""Martuni Schleuse analysis worker.

Polls remote outbox on martuni.de, analyzes PGNs locally with Stockfish /
Fairy-Stockfish, uploads results to inbox, updates status.json.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import signal
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

def _env(name: str, default: str) -> str:
    """Host-specific override; defaults keep the original Grok-Bot layout."""
    return os.environ.get(name, default)


REMOTE_HOST = _env("SCHLEUSE_REMOTE_HOST", "martuni.de")
REMOTE_DIR = _env("SCHLEUSE_REMOTE_DIR", "/home/librechat/grok_bot_schleuse")
PYTHON = _env("SCHLEUSE_PYTHON", "/workspace/enginemartuni/.venv/bin/python3")
ANALYZE_SCRIPT = _env("SCHLEUSE_ANALYZE_SCRIPT", "/workspace/schleuse-work/analyze_blunders.py")
WORK_DIR = Path(_env("SCHLEUSE_WORK_DIR", "/workspace/schleuse-work/jobs"))
LOG_PATH = Path(_env("SCHLEUSE_LOG_PATH", "/workspace/enginemartuni/logs/schleuse_worker.log"))
PID_PATH = Path(_env("SCHLEUSE_PID_PATH", "/workspace/enginemartuni/tools/schleuse_worker.pid"))
# Prepended to PATH so the engines are found (Debian puts them in /usr/games).
ENGINE_PATH = _env("SCHLEUSE_ENGINE_PATH", "/usr/games:/workspace/tools")

ENGINE_LABEL = _env("SCHLEUSE_ENGINE_LABEL", "Stockfish 17.1")
VARIANT_ENGINE_LABEL = _env("SCHLEUSE_VARIANT_ENGINE_LABEL", "Fairy-Stockfish 11.1 LB 64")

VANILLA_VARIANTS = {
    "",
    "standard",
    "chess",
    "normal",
    "from position",
    "fromposition",
    "chess960",
    "fischerandom",
    "fischerrandom",
}

VARIANT_RE = re.compile(r'\[Variant\s+"([^"]*)"\s*\]', re.IGNORECASE)

EMPTY_SLEEP_S = 300
DEPTH = 17
THREADS = 2
HASH = 256
MIN_MOVETIME = 0
# Hard ceiling per game (earlier jobs were 1–8 min; Racing Kings hung >5h without this)
ANALYZE_TIMEOUT_S = 45 * 60


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def log(msg: str) -> None:
    line = f"{utc_now_iso()} {msg}"
    LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
    with LOG_PATH.open("a", encoding="utf-8") as fh:
        fh.write(line + "\n")
    print(line, flush=True)


def ssh(remote_cmd: str, check: bool = True) -> subprocess.CompletedProcess:
    cmd = [
        "ssh",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=30",
        REMOTE_HOST,
        remote_cmd,
    ]
    return subprocess.run(cmd, capture_output=True, text=True, check=check)


def scp_from(remote_path: str, local_path: Path) -> None:
    # Modern OpenSSH scp (SFTP backend): host:path as ONE argv; do NOT shell-quote
    # the path — quotes become literal characters and the transfer fails.
    remote_spec = f"{REMOTE_HOST}:{remote_path}"
    cmd = [
        "scp",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=30",
        remote_spec,
        str(local_path),
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"scp_from failed rc={proc.returncode}: {(proc.stderr or proc.stdout or '').strip()}"
        )


def scp_to(local_path: Path, remote_path: str) -> None:
    remote_spec = f"{REMOTE_HOST}:{remote_path}"
    cmd = [
        "scp",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=30",
        str(local_path),
        remote_spec,
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"scp_to failed rc={proc.returncode}: {(proc.stderr or proc.stdout or '').strip()}"
        )


def remote_quote(path: str) -> str:
    """Shell-quote a remote path for use inside ssh remote_cmd."""
    return shlex.quote(path)


def acquire_pid_lock() -> bool:
    """Return True if we own the lock; False if another live instance exists."""
    PID_PATH.parent.mkdir(parents=True, exist_ok=True)
    if PID_PATH.exists():
        try:
            old_pid = int(PID_PATH.read_text(encoding="utf-8").strip())
        except (ValueError, OSError):
            old_pid = None
        if old_pid is not None:
            try:
                os.kill(old_pid, 0)
            except ProcessLookupError:
                pass  # stale
            except PermissionError:
                log(f"PID {old_pid} alive (permission); exiting 0")
                return False
            else:
                log(f"Another instance alive (pid={old_pid}); exiting 0")
                return False
    PID_PATH.write_text(str(os.getpid()) + "\n", encoding="utf-8")
    return True


def release_pid_lock() -> None:
    try:
        if PID_PATH.exists():
            cur = PID_PATH.read_text(encoding="utf-8").strip()
            if cur == str(os.getpid()):
                PID_PATH.unlink(missing_ok=True)
    except OSError:
        pass


def list_outbox_oldest_first() -> list[str]:
    """Return PGN basenames in outbox, oldest mtime first."""
    remote_cmd = (
        f"find {remote_quote(REMOTE_DIR + '/outbox')} -maxdepth 1 -type f -name '*.pgn' "
        f"-printf '%T@\\t%f\\0' | sort -z -n | cut -z -f2-"
    )
    proc = ssh(remote_cmd, check=True)
    raw = proc.stdout
    if not raw:
        return []
    names = [n for n in raw.split("\0") if n]
    return [n for n in names if n.endswith(".pgn")]


def list_inbox_done_names() -> set[str]:
    """Return set of PGN basenames that already have .json or .failed.json in inbox.

    Inbox artifacts are named '<pgn>.json' or '<pgn>.failed.json' where <pgn>
    includes the .pgn suffix (e.g. 'foo.pgn.json').
    """
    remote_cmd = (
        f"find {remote_quote(REMOTE_DIR + '/inbox')} -maxdepth 1 -type f "
        f"\\( -name '*.pgn.json' -o -name '*.pgn.failed.json' \\) -printf '%f\\0'"
    )
    proc = ssh(remote_cmd, check=True)
    raw = proc.stdout
    if not raw:
        return set()
    done: set[str] = set()
    for fname in raw.split("\0"):
        if not fname:
            continue
        if fname.endswith(".pgn.failed.json"):
            done.add(fname[: -len(".failed.json")])  # -> 'x.pgn'
        elif fname.endswith(".pgn.json"):
            done.add(fname[: -len(".json")])  # -> 'x.pgn'
    return done


def list_pending_oldest_first() -> list[str]:
    """Outbox PGNs oldest-first, excluding those with inbox result already."""
    outbox = list_outbox_oldest_first()
    if not outbox:
        return []
    done = list_inbox_done_names()
    pending = []
    stale = []
    for name in outbox:
        if name in done:
            stale.append(name)
        else:
            pending.append(name)
    # Drop stale outbox entries that Claude already has results for
    for name in stale:
        try:
            log(f"Skip already-done, deleting outbox: {name}")
            remote_delete_outbox(name)
        except Exception as exc:
            log(f"delete skip failed: {exc}")
    return pending


def queue_count() -> int:
    proc = ssh(
        f"find {remote_quote(REMOTE_DIR + '/outbox')} -maxdepth 1 -type f -name '*.pgn' | wc -l",
        check=True,
    )
    try:
        return int(proc.stdout.strip())
    except ValueError:
        return 0


def write_status(current: str | None, queue: int | None = None) -> None:
    if queue is None:
        try:
            queue = queue_count()
        except Exception as exc:
            log(f"queue_count failed for status: {exc}")
            queue = -1
    payload = {
        "last_run": utc_now_iso(),
        "engine": ENGINE_LABEL,
        "variant_engine": VARIANT_ENGINE_LABEL,
        "queue": queue,
        "current": current,
    }
    local_tmp = WORK_DIR / "status.json.tmp"
    local_final = WORK_DIR / "status.json"
    WORK_DIR.mkdir(parents=True, exist_ok=True)
    local_tmp.write_text(json.dumps(payload, ensure_ascii=False) + "\n", encoding="utf-8")
    local_final.write_text(local_tmp.read_text(encoding="utf-8"), encoding="utf-8")
    remote_tmp = f"{REMOTE_DIR}/status.json.tmp"
    remote_final = f"{REMOTE_DIR}/status.json"
    scp_to(local_tmp, remote_tmp)
    ssh(f"mv {remote_quote(remote_tmp)} {remote_quote(remote_final)}", check=True)
    local_tmp.unlink(missing_ok=True)


def detect_engine(pgn_text: str) -> str:
    m = VARIANT_RE.search(pgn_text)
    variant = (m.group(1) if m else "").strip().lower()
    # normalize spaces
    variant_norm = " ".join(variant.split())
    if variant_norm in VANILLA_VARIANTS or variant_norm.replace(" ", "") in {
        "fromposition",
        "fischerandom",
        "fischerrandom",
        "chess960",
    }:
        return "stockfish"
    if not variant_norm:
        return "stockfish"
    return "fairy-stockfish"


def remote_delete_outbox(pgn_name: str) -> None:
    path = f"{REMOTE_DIR}/outbox/{pgn_name}"
    ssh(f"rm -f {remote_quote(path)}", check=True)


def upload_inbox(local_file: Path, remote_basename: str) -> None:
    """Upload local_file as inbox/<remote_basename> via .tmp + mv."""
    remote_tmp = f"{REMOTE_DIR}/inbox/{remote_basename}.tmp"
    remote_final = f"{REMOTE_DIR}/inbox/{remote_basename}"
    scp_to(local_file, remote_tmp)
    ssh(f"mv {remote_quote(remote_tmp)} {remote_quote(remote_final)}", check=True)


def write_failed_and_upload(pgn_name: str, error: str, rc: int | None, engine: str) -> None:
    payload = {
        "error": error,
        "rc": rc,
        "engine": engine,
        "at": utc_now_iso(),
    }
    failed_name = f"{pgn_name}.failed.json"
    local_path = WORK_DIR / failed_name
    local_path.write_text(json.dumps(payload, ensure_ascii=False) + "\n", encoding="utf-8")
    upload_inbox(local_path, failed_name)


def process_one(pgn_name: str) -> None:
    log(f"Processing: {pgn_name!r}")
    # Race-safe skip (single SSH)
    check = ssh(
        f"if test -e {remote_quote(REMOTE_DIR + '/inbox/' + pgn_name + '.json')}; then echo json; "
        f"elif test -e {remote_quote(REMOTE_DIR + '/inbox/' + pgn_name + '.failed.json')}; then echo failed; "
        f"else echo no; fi",
        check=True,
    ).stdout.strip()
    if check in ("json", "failed"):
        log(f"Skip (inbox {check} exists): {pgn_name}")
        remote_delete_outbox(pgn_name)
        write_status(current=None)
        return

    write_status(current=pgn_name)

    WORK_DIR.mkdir(parents=True, exist_ok=True)
    local_pgn = WORK_DIR / pgn_name
    # Ensure fresh paths: remove any leftovers
    if local_pgn.exists():
        local_pgn.unlink()
    local_json = WORK_DIR / f"{pgn_name}.json"
    if local_json.exists():
        local_json.unlink()

    engine = "stockfish"
    try:
        scp_from(f"{REMOTE_DIR}/outbox/{pgn_name}", local_pgn)
        pgn_text = local_pgn.read_text(encoding="utf-8", errors="replace")
        engine = detect_engine(pgn_text)
        log(f"Engine selected: {engine} for {pgn_name!r}")

        env = os.environ.copy()
        env["PATH"] = f"{ENGINE_PATH}:{env.get('PATH', '')}"

        cmd = [
            PYTHON,
            ANALYZE_SCRIPT,
            "--engine",
            engine,
            "--depth",
            str(DEPTH),
            "--threads",
            str(THREADS),
            "--hash",
            str(HASH),
            "--min-movetime",
            str(MIN_MOVETIME),
            "--output",
            str(local_json),
            str(local_pgn),
        ]
        log(f"Running: {' '.join(shlex.quote(c) for c in cmd)}")
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                env=env,
                cwd=str(WORK_DIR),
                timeout=ANALYZE_TIMEOUT_S,
            )
        except subprocess.TimeoutExpired as exc:
            err = (
                f"analyze timed out after {ANALYZE_TIMEOUT_S}s "
                f"(engine={engine}); killed hung analysis"
            )
            log(f"TIMEOUT: {err}")
            # Best-effort: kill leftover engine children if any
            try:
                subprocess.run(
                    ["pkill", "-f", f"analyze_blunders.py.*{pgn_name}"],
                    check=False,
                )
            except Exception:
                pass
            write_failed_and_upload(pgn_name, err, None, engine)
            remote_delete_outbox(pgn_name)
            write_status(current=None)
            return
        if proc.returncode != 0:
            err = (proc.stderr or proc.stdout or f"rc={proc.returncode}").strip()
            if len(err) > 2000:
                err = err[:2000] + "…"
            log(f"FAIL rc={proc.returncode}: {err}")
            write_failed_and_upload(pgn_name, err, proc.returncode, engine)
            remote_delete_outbox(pgn_name)
            write_status(current=None)
            return

        if not local_json.exists():
            err = "analyze succeeded (rc=0) but output JSON missing"
            log(f"FAIL: {err}")
            write_failed_and_upload(pgn_name, err, 0, engine)
            remote_delete_outbox(pgn_name)
            write_status(current=None)
            return

        upload_inbox(local_json, f"{pgn_name}.json")
        remote_delete_outbox(pgn_name)
        log(f"OK uploaded inbox/{pgn_name}.json and deleted outbox")
        write_status(current=None)
    except Exception as exc:
        err = f"{type(exc).__name__}: {exc}"
        log(f"EXCEPTION: {err}")
        try:
            write_failed_and_upload(pgn_name, err, None, engine)
            remote_delete_outbox(pgn_name)
        except Exception as exc2:
            log(f"Failed to upload failure marker: {exc2}")
        try:
            write_status(current=None)
        except Exception as exc3:
            log(f"Failed to write status after exception: {exc3}")
    finally:
        # Keep local artifacts for debugging; optionally clean large ones later
        pass


def run_once() -> int:
    pending = list_pending_oldest_first()
    if not pending:
        log("Queue empty")
        write_status(current=None, queue=0)
        return 0
    process_one(pending[0])
    return 0


def run_loop() -> int:
    log("Entering loop mode (empty sleep 300s)")
    while True:
        try:
            pending = list_pending_oldest_first()
            if not pending:
                write_status(current=None, queue=0)
                log(f"Queue empty; sleeping {EMPTY_SLEEP_S}s")
                time.sleep(EMPTY_SLEEP_S)
                continue
            # Process continuously one-by-one while work available
            for name in pending:
                process_one(name)
        except Exception as exc:
            log(f"Loop error: {type(exc).__name__}: {exc}")
            time.sleep(60)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--once",
        action="store_true",
        help="Process at most one PGN then exit",
    )
    mode.add_argument(
        "--loop",
        action="store_true",
        help="Loop forever (default)",
    )
    args = parser.parse_args()
    do_once = bool(args.once)
    # default is loop
    if not args.once and not args.loop:
        do_once = False

    LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
    WORK_DIR.mkdir(parents=True, exist_ok=True)

    if not acquire_pid_lock():
        return 0

    def _cleanup(signum=None, frame=None):
        log(f"Signal {signum}; releasing lock and exiting")
        release_pid_lock()
        sys.exit(0)

    signal.signal(signal.SIGTERM, _cleanup)
    signal.signal(signal.SIGINT, _cleanup)

    log(f"schleuse_worker start pid={os.getpid()} once={do_once}")
    try:
        if do_once:
            return run_once()
        return run_loop()
    finally:
        release_pid_lock()


if __name__ == "__main__":
    sys.exit(main() or 0)
