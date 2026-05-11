#!/usr/bin/env python3
"""Cron job: analyse genau eine noch nicht verarbeitete PGN pro Lauf.

Der Cronjob ist als Gegenstück zum bisherigen "alles vor der Validierung
in einem Rutsch"-Workflow gedacht. Statt kurz vor dem Release eine lange
Stockfish-Last zu erzeugen, läuft `tools/analyze_blunders.py` jetzt
mehrfach pro Stunde mit jeweils nur einer PGN. Da die Zustandsdatei
(`--output`) additiv mitgepflegt wird, addieren sich die kleinen Läufe
zum gleichen Gesamtbild.

Ablauf pro Lauf:

  1. PID-Lock prüfen — läuft schon eine Instanz, wird kommentarlos
     beendet (mit Log-Hinweis). So überlappen langlaufende Analysen
     keinen späteren Cron-Tick.
  2. Config aus tools/analyze_cron.config.json lesen.
  3. Im konfigurierten `game-dir` alle *.pgn auflisten und alle Namen,
     die schon in der Zustandsdatei stehen, herausfiltern.
  4. Die älteste verbleibende PGN auswählen (chronologische Reihenfolge —
     so wandert der Analyse-Fortschritt zeitlich mit den Partien mit)
     und `analyze_blunders.py` exakt mit dieser einen Datei aufrufen.
  5. Lock freigeben.

Pfade in der Config werden relativ zum Repo-Root (Parent von tools/)
aufgelöst, damit derselbe Pfad funktioniert, egal von wo aus der Cron
gestartet wird.
"""

from __future__ import annotations

import json
import logging
import os
import subprocess
import sys
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
CONFIG_PATH = SCRIPT_DIR / "analyze_cron.config.json"
LOCK_PATH = SCRIPT_DIR / "analyze_cron.pid"
ANALYZE_SCRIPT = SCRIPT_DIR / "analyze_blunders.py"
PYTHON_BIN = REPO_ROOT / ".venv" / "bin" / "python3"
LOG_DIR = REPO_ROOT / "logs"
LOG_FILE = LOG_DIR / "analyze_cron.log"


def setup_logging() -> None:
    """Nur FileHandler — Cron-stdout/stderr darf separat woanders landen,
    der Hauptlog bleibt damit doppellung-frei und gut greppbar."""
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s  %(levelname)-7s  pid=%(process)d  %(message)s",
        handlers=[logging.FileHandler(LOG_FILE, encoding="utf-8")],
    )


# ---------------------------------------------------------------------------
# Lock (PID-File)
# ---------------------------------------------------------------------------
def _pid_alive(pid: int) -> bool:
    """True, falls Prozess existiert. `kill -0` ist signalfrei — kein Risiko."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # Prozess existiert, gehört aber jemand anderem — sicherheitshalber als "lebt" werten.
        return True
    return True


def acquire_lock() -> bool:
    """PID in Lock-File schreiben. False, wenn eine andere Instanz noch läuft."""
    if LOCK_PATH.exists():
        try:
            existing = int(LOCK_PATH.read_text().strip())
        except (ValueError, OSError):
            existing = None
        if existing is not None and _pid_alive(existing):
            logging.info("previous instance still alive (pid=%s) — exiting", existing)
            return False
        # Stale lock (Prozess weg, Datei blieb liegen) — überschreiben.
        logging.info("stale lock from pid=%s, replacing", existing)
    LOCK_PATH.write_text(str(os.getpid()))
    return True


def release_lock() -> None:
    try:
        # Nur entfernen, wenn er noch uns gehört — sonst läuft eine fremde
        # Instanz mit, deren Lock wir nicht freiräumen dürfen.
        if LOCK_PATH.exists() and LOCK_PATH.read_text().strip() == str(os.getpid()):
            LOCK_PATH.unlink()
    except OSError as e:
        logging.warning("could not remove lock %s: %s", LOCK_PATH, e)


# ---------------------------------------------------------------------------
# Config & State
# ---------------------------------------------------------------------------
def load_config() -> dict:
    with CONFIG_PATH.open("r", encoding="utf-8") as fh:
        return json.load(fh)


def load_analyzed(output_path: Path) -> set[str]:
    """Bereits verarbeitete PGN-Dateinamen aus der Zustandsdatei lesen.

    Format kommt aus analyze_blunders.save_state: {"analyzed_pgns": [...]}.
    Fehlt die Datei oder ist sie kaputt, geben wir eine leere Menge zurück —
    der erste Lauf legt sie ohnehin neu an.
    """
    if not output_path.exists():
        return set()
    try:
        with output_path.open("r", encoding="utf-8") as fh:
            data = json.load(fh)
        return set(data.get("analyzed_pgns", []))
    except (OSError, json.JSONDecodeError) as e:
        logging.warning("could not parse state file %s: %s — treating as empty", output_path, e)
        return set()


def pick_next_pgn(game_dir: Path, analyzed: set[str]) -> Path | None:
    """Älteste noch nicht analysierte *.pgn auswählen.

    Sortierung nach mtime: so wandern wir chronologisch durch die Partien.
    Alphabetisch wäre vom Lichess-ID-Header her quasi zufällig.
    """
    candidates = [p for p in game_dir.glob("*.pgn") if p.name not in analyzed]
    if not candidates:
        return None
    candidates.sort(key=lambda p: p.stat().st_mtime)
    return candidates[0]


def build_cmd(cfg: dict, pgn: Path) -> list[str]:
    """Kommandozeile für analyze_blunders.py aus der Config bauen.

    Wichtig: kein --game-dir hier — wir übergeben gezielt EINE PGN per
    Positional. analyze_blunders.py akzeptiert mehrere positionale Files,
    nutzt aber natürlich auch nur die übergebenen.
    """
    cmd: list[str] = [str(PYTHON_BIN), str(ANALYZE_SCRIPT)]
    cmd += ["--depth", str(int(cfg["depth"]))]
    cmd += ["--threads", str(int(cfg["threads"]))]
    cmd += ["--hash", str(int(cfg["hash"]))]
    cmd += ["--min-movetime", str(float(cfg["min-movetime"]))]
    cmd += ["--output", str(cfg["output"])]
    cmd += [str(pgn)]
    return cmd


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main() -> int:
    setup_logging()

    if not acquire_lock():
        return 0

    try:
        try:
            cfg = load_config()
        except (OSError, json.JSONDecodeError) as e:
            logging.error("could not load config %s: %s", CONFIG_PATH, e)
            return 1

        # Pfade in der Config sind relativ zum Repo-Root — dadurch ist das
        # Verhalten unabhängig vom CWD, mit dem Cron startet.
        game_dir = (REPO_ROOT / cfg["game-dir"]).resolve()
        if not game_dir.is_dir():
            logging.error("game-dir is not a directory: %s", game_dir)
            return 1

        output_path = (REPO_ROOT / cfg["output"]).resolve()
        analyzed = load_analyzed(output_path)

        pgn = pick_next_pgn(game_dir, analyzed)
        if pgn is None:
            logging.info(
                "no new PGN in %s (already analyzed: %d) — nothing to do",
                game_dir,
                len(analyzed),
            )
            return 0

        cmd = build_cmd(cfg, pgn)
        logging.info("starting analysis: %s (already done: %d)", pgn.name, len(analyzed))
        logging.info("cmd: %s", " ".join(cmd))

        start = time.monotonic()
        # check=False: nicht-null-rc loggen wir selbst, statt eine Exception
        # zu werfen — Cron soll keinen Stack-Trace per Mail verschicken.
        proc = subprocess.run(cmd, cwd=REPO_ROOT, check=False)
        dur = time.monotonic() - start

        logging.info("done %s in %.1fs (rc=%d)", pgn.name, dur, proc.returncode)
        return 0 if proc.returncode == 0 else proc.returncode

    finally:
        release_lock()


if __name__ == "__main__":
    sys.exit(main())
