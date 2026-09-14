#!/usr/bin/env python3
"""Schleusen-Sync: Blunder-Analyse an den Grok-Bot auslagern.

Seit 14.09.2026 rechnet dieser Server keine Stockfish-Analysen mehr selbst
(`tools/analyze_cron.py` ist abgeschaltet, der Server ist am Limit). An seine
Stelle tritt dieses Skript, das nur noch Dateien hin- und herschiebt:

  Martuni-Server                    grok_bot_schleuse/            Grok-Bot
  ─────────────────────────────     ────────────────────────      ─────────────────
  offene PGNs kopieren        ──►   outbox/<pgn>.pgn        ──►   analyze_blunders.py
  Ergebnis in analyse-*.json  ◄──   inbox/<pgn>.json        ◄──   (Stockfish /
  mergen, Datei nach done/          inbox/<pgn>.failed.json ◄──    Fairy-Stockfish)

Das Protokoll zwischen beiden Seiten steht in docs/grok-schleuse.md (und als
Kopie in grok_bot_schleuse/PROTOKOLL.md). Kurzfassung:

  * outbox/  — schreibt nur dieser Server. Eine Datei = eine zu analysierende
               Partie, Dateiname identisch mit game_records/. Der Grok-Bot
               löscht die Datei, sobald sein Ergebnis in inbox/ liegt.
  * inbox/   — schreibt nur der Grok-Bot. `<pgn>.json` ist exakt die
               Zustandsdatei, die `analyze_blunders.py --output` für genau
               diese eine PGN erzeugt ({"version","updated_at",
               "analyzed_pgns":[<pgn>],"blunders":[...]}). Bei Fehlschlag
               stattdessen `<pgn>.failed.json` mit {"error": "...", "rc": n}.
  * done/    — Archiv der verarbeiteten inbox-Dateien (schreibt dieser Server).

Ablauf pro Lauf (Cron, alle 10 Minuten):

  1. PID-Lock wie beim alten Cron.
  2. Import: alle inbox/*.json einlesen, in die passende Zustandsdatei
     (Standard/Chess960 → `output`, echte Varianten → `variant-output`)
     mergen, Datei nach done/ verschieben, Outbox-Kopie entfernen.
     `.failed.json` erhöht den Quarantäne-Zähler (max-failures wie bisher),
     die PGN wird beim nächsten Export erneut angeboten — bis Quarantäne.
  3. Export: alle PGNs aus game_records/, die weder analysiert noch in
     Quarantäne noch bereits in outbox/ oder inbox/ sind, chronologisch
     (mtime) nach outbox/ kopieren; höchstens `max-outbox` Dateien gleichzeitig.

Die Zustandsdateien behalten exakt das Format von analyze_blunders.save_state,
deshalb funktionieren `~/bin/info`, `analyze_blunders.py --report` und die
Archivierungsroutine unverändert. Config ist dieselbe wie beim alten Cron
(tools/analyze_cron.config.json), neu sind nur die optionalen Schlüssel
`schleuse-dir` (Default ~/grok_bot_schleuse) und `max-outbox` (Default 50).
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

# Die Hilfsfunktionen des alten Crons (Lock, Quarantäne, Variantenerkennung)
# werden wiederverwendet — eine Logik, eine Wahrheit.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import analyze_cron as ac  # noqa: E402

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
DEFAULT_SCHLEUSE = Path.home() / "grok_bot_schleuse"
DEFAULT_MAX_OUTBOX = 50
LOG_FILE = REPO_ROOT / "logs" / "schleuse_sync.log"
LOCK_PATH = SCRIPT_DIR / "schleuse_sync.pid"

FAILED_SUFFIX = ".failed.json"


def setup_logging() -> None:
    LOG_FILE.parent.mkdir(parents=True, exist_ok=True)
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s  %(levelname)-7s  pid=%(process)d  %(message)s",
        handlers=[logging.FileHandler(LOG_FILE, encoding="utf-8")],
    )


# ---------------------------------------------------------------------------
# Zustandsdateien (Format: analyze_blunders.save_state)
# ---------------------------------------------------------------------------
def load_state(path: Path) -> dict:
    if not path.exists():
        return {"version": 1, "analyzed_pgns": [], "blunders": []}
    with path.open("r", encoding="utf-8") as fh:
        data = json.load(fh)
    data.setdefault("version", 1)
    data.setdefault("analyzed_pgns", [])
    data.setdefault("blunders", [])
    return data


def save_state(path: Path, data: dict) -> None:
    """Atomar schreiben — nie eine halbe JSON hinterlassen (Lehre vom 06.09.)."""
    out = {
        "version": data.get("version", 1),
        "updated_at": datetime.now(timezone.utc).isoformat(),
        "analyzed_pgns": sorted(set(data["analyzed_pgns"])),
        "blunders": data["blunders"],
    }
    tmp = path.with_suffix(".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=2)
    tmp.replace(path)


def merge_result(state: dict, result: dict) -> tuple[int, int]:
    """Ein Einzel-Ergebnis in den Gesamtzustand einarbeiten.

    Dedupe über (game_id, ply): läuft ein Ergebnis versehentlich zweimal durch
    die Schleuse, verdoppeln sich die Blunder nicht. Rückgabe: (neue PGNs,
    neue Blunder).
    """
    known = {(b.get("game_id"), b.get("ply")) for b in state["blunders"]}
    before_pgns = set(state["analyzed_pgns"])
    new_pgns = [n for n in result.get("analyzed_pgns", []) if n not in before_pgns]
    state["analyzed_pgns"] = sorted(before_pgns | set(new_pgns))
    added = 0
    for b in result.get("blunders", []):
        key = (b.get("game_id"), b.get("ply"))
        if key in known:
            continue
        known.add(key)
        state["blunders"].append(b)
        added += 1
    return len(new_pgns), added


def move_to_done(path: Path, done_dir: Path) -> None:
    done_dir.mkdir(parents=True, exist_ok=True)
    target = done_dir / path.name
    if target.exists():
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S")
        target = done_dir / f"{path.stem}.{stamp}{path.suffix}"
    shutil.move(str(path), str(target))


def variant_output_for(pgn_name: str, game_dir: Path, result: dict) -> bool:
    """True, wenn das Ergebnis in die Varianten-Zustandsdatei gehört.

    Primär aus dem lokalen PGN-Header (wie der alte Cron); ist die PGN hier
    schon archiviert, zählt ein optionales "variant"-Feld im Ergebnis.
    """
    pgn = game_dir / pgn_name
    if pgn.exists():
        return ac.needs_variant_engine(ac.read_pgn_variant(pgn))
    variant = str(result.get("variant", "standard")).strip().lower()
    return ac.needs_variant_engine(variant)


# ---------------------------------------------------------------------------
# Import: inbox/ → Zustandsdateien
# ---------------------------------------------------------------------------
def import_results(
    inbox: Path,
    outbox: Path,
    done: Path,
    game_dir: Path,
    output_path: Path,
    variant_output_path: Path,
    failures: dict,
    max_failures: int,
    dry_run: bool,
) -> tuple[int, int]:
    """Alle Ergebnisse aus inbox/ mergen. Rückgabe (importiert, fehlgeschlagen)."""
    files = sorted(p for p in inbox.glob("*.json") if not p.name.endswith(".tmp"))
    if not files:
        return 0, 0

    states = {
        output_path: load_state(output_path),
        variant_output_path: load_state(variant_output_path),
    }
    dirty: set[Path] = set()
    imported = failed = 0

    for res in files:
        if res.name.endswith(FAILED_SUFFIX):
            pgn_name = res.name[: -len(FAILED_SUFFIX)]
            try:
                info = json.loads(res.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                info = {}
            reason = str(info.get("error", "grok-bot: analysis failed"))[:300]
            rc = info.get("rc")
            logging.warning("FAILED %s: rc=%s %s", pgn_name, rc, reason)
            failed += 1
            if dry_run:
                continue
            ac.record_failure(failures, pgn_name, rc, reason, max_failures)
            move_to_done(res, done)
            # Outbox-Kopie weg — der nächste Export bietet die PGN erneut an
            # (außer sie ist inzwischen in Quarantäne).
            (outbox / pgn_name).unlink(missing_ok=True)
            continue

        pgn_name = res.name[: -len(".json")]
        try:
            result = json.loads(res.read_text(encoding="utf-8"))
        except (OSError, ValueError) as e:
            # Vermutlich gerade erst halb geschrieben — nächster Tick.
            logging.warning("inbox %s nicht lesbar (%s) — übersprungen", res.name, e)
            continue
        if not isinstance(result, dict) or not isinstance(result.get("blunders"), list):
            logging.error("inbox %s hat kein Zustandsformat — nach done/ verschoben", res.name)
            if not dry_run:
                move_to_done(res, done)
            continue
        result.setdefault("analyzed_pgns", [pgn_name])
        if pgn_name not in result["analyzed_pgns"]:
            # Name der Ergebnisdatei ist die Wahrheit; analyzed_pgns hält der
            # Grok-Bot normalerweise identisch, aber lieber robust.
            result["analyzed_pgns"].append(pgn_name)

        target = variant_output_path if variant_output_for(pgn_name, game_dir, result) else output_path
        n_pgn, n_bl = merge_result(states[target], result)
        logging.info(
            "IMPORT %s → %s (+%d PGN, +%d Blunder)", pgn_name, target.name, n_pgn, n_bl
        )
        imported += 1
        if dry_run:
            continue
        dirty.add(target)
        if failures.pop(pgn_name, None) is not None:
            logging.info("frühere Fehlschläge für %s zurückgesetzt", pgn_name)
        move_to_done(res, done)
        (outbox / pgn_name).unlink(missing_ok=True)

    if not dry_run:
        for path in dirty:
            save_state(path, states[path])
    return imported, failed


# ---------------------------------------------------------------------------
# Export: game_records/ → outbox/
# ---------------------------------------------------------------------------
def export_pending(
    game_dir: Path,
    outbox: Path,
    inbox: Path,
    analyzed: set[str],
    quarantined: set[str],
    max_outbox: int,
    dry_run: bool,
) -> tuple[int, int]:
    """Offene PGNs nach outbox/ kopieren. Rückgabe (exportiert, noch wartend)."""
    in_outbox = {p.name for p in outbox.glob("*.pgn")}
    in_inbox = {p.name[: -len(".json")] for p in inbox.glob("*.pgn.json")}
    in_inbox |= {p.name[: -len(FAILED_SUFFIX)] for p in inbox.glob("*" + FAILED_SUFFIX)}
    skip = analyzed | quarantined | in_outbox | in_inbox

    pending = [p for p in game_dir.glob("*.pgn") if p.name not in skip]
    pending.sort(key=lambda p: p.stat().st_mtime)

    room = max(0, max_outbox - len(in_outbox))
    to_send = pending[:room]
    for pgn in to_send:
        logging.info("EXPORT %s", pgn.name)
        if dry_run:
            continue
        # tmp + rename: der Grok-Bot sieht nie eine halb kopierte PGN.
        tmp = outbox / (pgn.name + ".tmp")
        shutil.copy2(pgn, tmp)
        tmp.replace(outbox / pgn.name)
    return len(to_send), len(pending) - len(to_send)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--config", type=Path, default=ac.CONFIG_PATH)
    ap.add_argument("--schleuse", type=Path, help="überschreibt schleuse-dir aus der Config")
    ap.add_argument("--dry-run", action="store_true", help="nur loggen, nichts schreiben")
    args = ap.parse_args()

    setup_logging()

    # Eigener Lock (nicht der von analyze_cron), damit ein versehentlich
    # noch laufender alter Analyse-Tick uns nicht blockiert und umgekehrt.
    ac.LOCK_PATH = LOCK_PATH
    if not ac.acquire_lock():
        return 0
    try:
        try:
            cfg = json.loads(args.config.read_text(encoding="utf-8"))
        except (OSError, ValueError) as e:
            logging.error("could not load config %s: %s", args.config, e)
            return 1

        game_dir = (REPO_ROOT / cfg["game-dir"]).resolve()
        if not game_dir.is_dir():
            logging.error("game-dir is not a directory: %s", game_dir)
            return 1

        schleuse = args.schleuse or Path(os.path.expanduser(cfg.get("schleuse-dir", str(DEFAULT_SCHLEUSE))))
        if not schleuse.is_dir():
            logging.error("schleuse-dir is not a directory: %s", schleuse)
            return 1
        outbox, inbox, done = schleuse / "outbox", schleuse / "inbox", schleuse / "done"
        if not args.dry_run:
            for d in (outbox, inbox, done):
                d.mkdir(parents=True, exist_ok=True)

        output_name = str(cfg["output"])
        variant_output_name = str(cfg.get("variant-output") or ac.default_variant_output(output_name))
        output_path = (REPO_ROOT / output_name).resolve()
        variant_output_path = (REPO_ROOT / variant_output_name).resolve()
        max_failures = int(cfg.get("max-failures", ac.DEFAULT_MAX_FAILURES))
        max_outbox = int(cfg.get("max-outbox", DEFAULT_MAX_OUTBOX))

        failures = ac.load_quarantine()

        imported, failed = import_results(
            inbox, outbox, done, game_dir, output_path, variant_output_path,
            failures, max_failures, args.dry_run,
        )
        if (imported or failed) and not args.dry_run:
            ac.save_quarantine(failures, game_dir)

        analyzed = ac.load_analyzed(output_path) | ac.load_analyzed(variant_output_path)
        quarantined = ac.quarantined_names(failures, max_failures)
        exported, waiting = export_pending(
            game_dir, outbox, inbox, analyzed, quarantined, max_outbox, args.dry_run,
        )

        logging.info(
            "SUMMARY%s: imported=%d failed=%d exported=%d | outbox=%d inbox=%d waiting=%d | analyzed=%d quarantined=%d",
            " (dry-run)" if args.dry_run else "",
            imported, failed, exported,
            len(list(outbox.glob("*.pgn"))) if outbox.is_dir() else 0,
            len(list(inbox.glob("*.json"))) if inbox.is_dir() else 0,
            waiting, len(analyzed), len(quarantined),
        )
        return 0
    finally:
        ac.release_lock()


if __name__ == "__main__":
    sys.exit(main())
