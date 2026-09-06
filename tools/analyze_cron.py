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
  3. Im konfigurierten `game-dir` alle *.pgn auflisten und alle Namen
     herausfiltern, die schon in einer der Zustandsdateien stehen oder
     in Quarantäne sind.
  4. Die älteste verbleibende PGN auswählen (chronologische Reihenfolge —
     so wandert der Analyse-Fortschritt zeitlich mit den Partien mit),
     anhand ihres `[Variant]`-Headers Engine und Zustandsdatei wählen
     und `analyze_blunders.py` exakt mit dieser einen Datei aufrufen.
  5. Ergebnis verbuchen (Fehlerzähler hoch bzw. zurücksetzen), Lock freigeben.

Zwei Analysepfade
-----------------
Standard, Chess960 und "From Position" gehen an die normale Stockfish-Binary
und in die Zustandsdatei `output`. Echte Varianten (Antichess, Atomic,
Crazyhouse, King of the Hill, Horde, Three-check, Racing Kings) kann
Stockfish nicht — die gehen an `variant-engine` (Fairy-Stockfish) und in
die getrennte Zustandsdatei `variant-output`. Getrennt deshalb, weil ein
Antichess-Blunder statistisch nichts mit einem Standard-Blunder zu tun hat
und die gewohnten Blunder/Partie-Kennzahlen sonst unbrauchbar würden.

Quarantäne (Sicherheitsnetz)
----------------------------
`analyze_blunders.py` verbucht eine PGN nur bei Erfolg in der Zustandsdatei.
Scheitert sie, wählt `pick_next_pgn()` beim nächsten Tick wieder dieselbe
Datei — und blockiert damit alle jüngeren Partien dahinter (Head-of-Line).
Genau das passierte am 05./06.09.2026: Varianten-PGNs ließen den
variantenunfähigen Stockfish mit `EngineError: engine does not support
UCI_Variant` sterben, jede Minute aufs Neue. Darum zählt
`tools/analyze_cron.quarantine.json` Fehlschläge pro Datei mit; ab
`max-failures` wird die PGN dauerhaft übersprungen. Ein späterer
Erfolgslauf löscht den Zähler wieder.

Pfade in der Config werden relativ zum Repo-Root (Parent von tools/)
aufgelöst, damit derselbe Pfad funktioniert, egal von wo aus der Cron
gestartet wird.
"""

from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
CONFIG_PATH = SCRIPT_DIR / "analyze_cron.config.json"
LOCK_PATH = SCRIPT_DIR / "analyze_cron.pid"
ANALYZE_SCRIPT = SCRIPT_DIR / "analyze_blunders.py"
PYTHON_BIN = REPO_ROOT / ".venv" / "bin" / "python3"
LOG_DIR = REPO_ROOT / "logs"
LOG_FILE = LOG_DIR / "analyze_cron.log"
QUARANTINE_PATH = SCRIPT_DIR / "analyze_cron.quarantine.json"

# Varianten, die die normale Stockfish-Binary beherrscht. Chess960 läuft über
# UCI_Chess960, "From Position" ist normales Schach mit anderer Startstellung —
# beides also kein Fall für Fairy-Stockfish. Alles andere schon.
VANILLA_VARIANTS = {
    "",
    "standard",
    "chess",
    "normal",
    "from position",
    "chess960",
    "fischerandom",
    "fischerrandom",
}

DEFAULT_ENGINE = "stockfish"
DEFAULT_VARIANT_ENGINE = "fairy-stockfish"
DEFAULT_MAX_FAILURES = 3

# Cron startet mit minimalem PATH (/usr/bin:/bin). `stockfish` liegt unter
# /usr/games, `fairy-stockfish` unter ~/tools — ohne diese Ergänzung findet
# python-chess die Engines nicht und analyze_blunders.py bricht mit rc=2 ab.
ENGINE_DIRS = ["/usr/games", str(REPO_ROOT.parent / "tools")]


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


def default_variant_output(output: str) -> str:
    """Ableitung der Varianten-Zustandsdatei aus der Standard-Zustandsdatei.

    "analyse-05.10.2026.json" -> "analyse-05.10.2026-varianten.json".
    Nur Fallback — steht `variant-output` in der Config, gilt die.
    """
    path = Path(output)
    return str(path.with_name(f"{path.stem}-varianten{path.suffix}"))


def read_pgn_variant(pgn: Path) -> str:
    """Variantennamen aus dem PGN-Header lesen, klein und ohne Anführungszeichen.

    Wir lesen nur den Header-Block bis zur ersten Leerzeile — das ist billig
    und reicht, um die passende Analyse-Engine zu wählen. Fehlt der Header
    (oder ist er kaputt), behandeln wir die Partie als Standardschach; das
    ist die Annahme, mit der der Cron vor der Varianten-Ära gelaufen ist.
    """
    try:
        with pgn.open("r", encoding="utf-8", errors="replace") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    break  # Leerzeile = Ende des Header-Blocks
                if line.startswith("[Variant "):
                    parts = line.split('"')
                    if len(parts) >= 2:
                        return parts[1].strip().lower()
    except OSError as e:
        logging.warning("could not read variant header from %s: %s", pgn.name, e)
    return "standard"


def needs_variant_engine(variant: str) -> bool:
    """True, wenn die Partie eine variantenfähige Engine braucht."""
    return variant not in VANILLA_VARIANTS


# ---------------------------------------------------------------------------
# Quarantäne — Sicherheitsnetz gegen Head-of-Line-Blocking
# ---------------------------------------------------------------------------
def load_quarantine() -> dict[str, dict]:
    """Fehlerzähler laden. Kaputte/fehlende Datei = leerer Zustand."""
    if not QUARANTINE_PATH.exists():
        return {}
    try:
        with QUARANTINE_PATH.open("r", encoding="utf-8") as fh:
            data = json.load(fh)
        failures = data.get("failures", {})
        return failures if isinstance(failures, dict) else {}
    except (OSError, json.JSONDecodeError) as e:
        logging.warning(
            "could not parse quarantine file %s: %s — treating as empty", QUARANTINE_PATH, e
        )
        return {}


def save_quarantine(failures: dict[str, dict], game_dir: Path) -> None:
    """Fehlerzähler schreiben; Einträge zu verschwundenen PGNs fallen raus.

    Nach einer Archivierung ist game_records/ leergeräumt — ohne dieses
    Aufräumen würde die Datei über die Monate mit Karteileichen zuwachsen.
    Geschrieben wird über eine Temp-Datei mit `replace()`, damit ein
    Abbruch mitten im Schreiben keine halbe JSON hinterlässt (genau der
    Fehlermodus, der am 06.09.2026 die Config zerlegt hat).
    """
    pruned = {name: info for name, info in failures.items() if (game_dir / name).exists()}
    payload = {
        "version": 1,
        "updated_at": datetime.now(timezone.utc).isoformat(),
        "failures": pruned,
    }
    tmp = QUARANTINE_PATH.with_suffix(".json.tmp")
    try:
        tmp.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        tmp.replace(QUARANTINE_PATH)
    except OSError as e:
        logging.warning("could not write quarantine file %s: %s", QUARANTINE_PATH, e)


def quarantined_names(failures: dict[str, dict], max_failures: int) -> set[str]:
    """Namen der PGNs, die endgültig übersprungen werden."""
    return {
        name
        for name, info in failures.items()
        if int(info.get("count", 0)) >= max_failures
    }


def record_failure(
    failures: dict[str, dict], name: str, rc: int | None, reason: str, max_failures: int
) -> None:
    """Fehlschlag verbuchen und ab `max_failures` in Quarantäne schicken."""
    entry = failures.setdefault(name, {"count": 0})
    entry["count"] = int(entry.get("count", 0)) + 1
    entry["rc"] = rc
    entry["reason"] = reason
    entry["last_failure"] = datetime.now(timezone.utc).isoformat()
    if entry["count"] >= max_failures:
        logging.error(
            "QUARANTINE %s nach %d Fehlschlägen (rc=%s, %s) — wird künftig übersprungen",
            name, entry["count"], rc, reason,
        )
    else:
        logging.warning(
            "Fehlschlag %d/%d für %s (rc=%s, %s) — wird erneut versucht",
            entry["count"], max_failures, name, rc, reason,
        )


def pick_next_pgn(game_dir: Path, skip: set[str]) -> Path | None:
    """Älteste noch nicht analysierte *.pgn auswählen.

    `skip` enthält alles, was nicht mehr drankommen soll: bereits analysierte
    PGNs (aus beiden Zustandsdateien) und die Quarantäne-Fälle.

    Sortierung nach mtime: so wandern wir chronologisch durch die Partien.
    Alphabetisch wäre vom Lichess-ID-Header her quasi zufällig.
    """
    candidates = [p for p in game_dir.glob("*.pgn") if p.name not in skip]
    if not candidates:
        return None
    candidates.sort(key=lambda p: p.stat().st_mtime)
    return candidates[0]


def build_cmd(cfg: dict, pgn: Path, engine: str, output: str) -> list[str]:
    """Kommandozeile für analyze_blunders.py aus der Config bauen.

    `engine` und `output` kommen von außen, weil beide von der Variante der
    Partie abhängen (Stockfish/Fairy-Stockfish, Standard-/Varianten-Zustand).

    Wichtig: kein --game-dir hier — wir übergeben gezielt EINE PGN per
    Positional. analyze_blunders.py akzeptiert mehrere positionale Files,
    nutzt aber natürlich auch nur die übergebenen.
    """
    cmd: list[str] = [str(PYTHON_BIN), str(ANALYZE_SCRIPT)]
    cmd += ["--engine", engine]
    cmd += ["--depth", str(int(cfg["depth"]))]
    cmd += ["--threads", str(int(cfg["threads"]))]
    cmd += ["--hash", str(int(cfg["hash"]))]
    cmd += ["--min-movetime", str(float(cfg["min-movetime"]))]
    cmd += ["--output", output]
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

        # PATH einmal zentral um die Engine-Verzeichnisse ergänzen — wird
        # sowohl für den which()-Check als auch für den Subprozess gebraucht.
        env = os.environ.copy()
        path_parts = [d for d in env.get("PATH", "").split(os.pathsep) if d]
        for engine_dir in reversed(ENGINE_DIRS):
            if engine_dir not in path_parts:
                path_parts.insert(0, engine_dir)
        env["PATH"] = os.pathsep.join(path_parts)

        # Zwei getrennte Zustandsdateien (siehe Modul-Docstring): Standard und
        # Chess960 wie bisher in `output`, echte Varianten in `variant-output`.
        output_name = str(cfg["output"])
        variant_output_name = str(cfg.get("variant-output") or default_variant_output(output_name))
        output_path = (REPO_ROOT / output_name).resolve()
        variant_output_path = (REPO_ROOT / variant_output_name).resolve()

        # Vereinigung beider Zustände: eine PGN gilt als erledigt, egal in
        # welchem der beiden Pfade sie gelandet ist.
        analyzed = load_analyzed(output_path) | load_analyzed(variant_output_path)

        max_failures = int(cfg.get("max-failures", DEFAULT_MAX_FAILURES))
        failures = load_quarantine()
        quarantined = quarantined_names(failures, max_failures)

        pgn = pick_next_pgn(game_dir, analyzed | quarantined)
        if pgn is None:
            logging.info(
                "no new PGN in %s (already analyzed: %d, quarantined: %d) — nothing to do",
                game_dir,
                len(analyzed),
                len(quarantined),
            )
            return 0

        variant = read_pgn_variant(pgn)
        if needs_variant_engine(variant):
            engine = str(cfg.get("variant-engine") or DEFAULT_VARIANT_ENGINE)
            output = variant_output_name
        else:
            engine = str(cfg.get("engine") or DEFAULT_ENGINE)
            output = output_name

        # Fehlende Engine würde sonst nur als nichtssagendes rc=2 durchlaufen
        # und die PGN nach `max-failures` grundlos in Quarantäne schicken.
        # Lieber einmal deutlich ins Log schreiben, was zu installieren ist.
        if shutil.which(engine, path=env["PATH"]) is None:
            logging.error(
                "engine '%s' nicht gefunden (variant=%s, PATH=%s) — %s übersprungen",
                engine,
                variant,
                env["PATH"],
                pgn.name,
            )
            record_failure(failures, pgn.name, None, f"engine '{engine}' not found", max_failures)
            save_quarantine(failures, game_dir)
            return 1

        cmd = build_cmd(cfg, pgn, engine, output)
        logging.info(
            "starting analysis: %s (variant=%s, engine=%s, output=%s, already done: %d, quarantined: %d)",
            pgn.name,
            variant,
            engine,
            output,
            len(analyzed),
            len(quarantined),
        )
        logging.info("cmd: %s", " ".join(cmd))

        start = time.monotonic()
        # check=False: nicht-null-rc loggen wir selbst, statt eine Exception
        # zu werfen — Cron soll keinen Stack-Trace per Mail verschicken.
        proc = subprocess.run(cmd, cwd=REPO_ROOT, env=env, check=False)
        dur = time.monotonic() - start

        logging.info("done %s in %.1fs (rc=%d)", pgn.name, dur, proc.returncode)

        # Erfolg löscht einen etwaigen Zähler wieder (transiente Fehler sollen
        # sich nicht über Wochen zu einer Quarantäne aufaddieren), Misserfolg
        # zählt hoch — sonst wählt der nächste Tick dieselbe Datei erneut.
        if proc.returncode == 0:
            if failures.pop(pgn.name, None) is not None:
                logging.info("frühere Fehlschläge für %s zurückgesetzt", pgn.name)
                save_quarantine(failures, game_dir)
        else:
            record_failure(
                failures, pgn.name, proc.returncode, "analyze_blunders.py failed", max_failures
            )
            save_quarantine(failures, game_dir)

        return 0 if proc.returncode == 0 else proc.returncode

    finally:
        release_lock()


if __name__ == "__main__":
    sys.exit(main())
