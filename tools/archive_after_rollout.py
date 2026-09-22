#!/usr/bin/env python3
"""archive_after_rollout.py — sauberer Schnitt nach einem Engine-Rollout.

Warum: Nach jedem Rollout sollen die Partien, die in die Auswertung
eingeflossen sind, aus `game_records/` verschwinden, und alles, was danach
kommt, klar als "neue Partien" erkennbar sein — im Ordner wie in der
Analysedatei. Der Ablauf (bisher von Hand, seit 22.09.2026 hier):

  1. Cut bestimmen: `--cut "YYYY-MM-DD HH:MM[:SS]"` (lokale Zeit) oder, wenn
     nicht angegeben, die mtime des Live-Binaries `target/release/martuni`
     — ab diesem Moment starten neue Partien mit dem neuen Binary
     (lichess-bot startet die Engine pro Partie).
  2. Alte Partien = PGNs in `game_records/`, deren Startzeit (Header
     UTCDate/UTCTime, hilfsweise Datei-mtime) VOR dem Cut liegt.
  3. Drain: solange alte Partien weder analysiert noch in Quarantäne sind
     (Schleuse noch unterwegs), `schleuse_sync.py` laufen lassen und warten
     (Poll alle 60 s, höchstens `--timeout` Minuten). So landen die letzten
     Ergebnisse noch in der ALTEN Analysedatei. Mit `--force` wird nicht
     gewartet (spätere Ergebnisse werden dann in die NEUE Datei gemerged —
     die Schleuse ordnet sie per `variant`-Feld zu).
  4. Alte PGNs nach `game_archiv/` (neben `game_records/`) verschieben.
  5. Alte Analysedateien (`output`, `variant-output` aus der Config) nach
     `archiv/` verschieben.
  6. Config `tools/analyze_cron.config.json`: `output` und `variant-output`
     auf `analyse-<DD.MM.YYYY>.json` / `analyse-<DD.MM.YYYY>-varianten.json`
     umstellen (Datum = `--date` oder heute). Vor dem Ersetzen wird das
     Ergebnis mit `json.loads` validiert (Lehre vom 06.09.2026: eine kaputte
     Config legt die Schleuse still), Backup `*.bak-<stamp>` bleibt liegen.
  7. Quarantäne-Einträge der verschobenen PGNs entfernen; `done/` der
     Schleuse in einen Unterordner `bis-<DD.MM.YYYY>/` wegräumen.

Ohne `--execute` ist alles ein Probelauf (nur Ausgabe). Während der Schritte
4–7 hält das Skript den Lock der Schleuse (derselbe wie `schleuse_sync.py`),
damit kein Cron-Tick dazwischenfunkt.

Beispiel:
  .venv/bin/python3 tools/archive_after_rollout.py            # Probelauf
  .venv/bin/python3 tools/archive_after_rollout.py --execute  # scharf
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import analyze_cron as ac  # noqa: E402

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
LIVE_BINARY = REPO_ROOT / "target" / "release" / "martuni"
ARCHIV_DIR = REPO_ROOT / "archiv"
SYNC_SCRIPT = SCRIPT_DIR / "schleuse_sync.py"
DEFAULT_SCHLEUSE = Path.home() / "grok_bot_schleuse"
HDR = re.compile(r'^\[(\w+) "([^"]*)"\]', re.M)


def log(msg: str) -> None:
    print(f"[{datetime.now():%H:%M:%S}] {msg}", flush=True)


def game_start_utc(pgn: Path) -> datetime:
    """Startzeit der Partie (UTC) aus dem Header; Fallback: Datei-mtime."""
    try:
        head = pgn.read_text(encoding="utf-8", errors="replace")[:3000]
        h = dict(HDR.findall(head))
        return datetime.strptime(
            f"{h['UTCDate']} {h['UTCTime']}", "%Y.%m.%d %H:%M:%S"
        ).replace(tzinfo=timezone.utc)
    except (KeyError, ValueError, OSError):
        return datetime.fromtimestamp(pgn.stat().st_mtime, tz=timezone.utc)


def parse_cut(text: str | None) -> datetime:
    if text:
        for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"):
            try:
                return datetime.strptime(text, fmt).astimezone()
            except ValueError:
                continue
        sys.exit(f"--cut nicht lesbar: {text!r} (erwartet 'YYYY-MM-DD HH:MM[:SS]', lokale Zeit)")
    if not LIVE_BINARY.exists():
        sys.exit(f"Live-Binary fehlt: {LIVE_BINARY} — bitte --cut angeben")
    return datetime.fromtimestamp(LIVE_BINARY.stat().st_mtime).astimezone()


def unique_target(dir_: Path, name: str) -> Path:
    target = dir_ / name
    if not target.exists():
        return target
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    p = Path(name)
    return dir_ / f"{p.stem}.{stamp}{p.suffix}"


def run_sync() -> None:
    """Einen Schleusen-Tick auslösen (importiert inbox, exportiert offene PGNs)."""
    py = REPO_ROOT / ".venv" / "bin" / "python3"
    cmd = [str(py if py.exists() else sys.executable), str(SYNC_SCRIPT)]
    subprocess.run(cmd, cwd=REPO_ROOT, check=False, timeout=600)


def rewrite_config(cfg_path: Path, new_output: str, new_variant: str, execute: bool) -> None:
    """Nur die beiden Werte ersetzen (Formatierung/Kommentare bleiben), dann validieren."""
    text = cfg_path.read_text(encoding="utf-8")
    new_text, n = re.subn(r'("output"\s*:\s*")[^"]*(")', rf'\g<1>{new_output}\g<2>', text, count=1)
    if n != 1:
        sys.exit("Config: Schlüssel \"output\" nicht gefunden — bitte von Hand prüfen")
    if re.search(r'"variant-output"\s*:', new_text):
        new_text, _ = re.subn(r'("variant-output"\s*:\s*")[^"]*(")', rf'\g<1>{new_variant}\g<2>', new_text, count=1)
    else:
        new_text = new_text.replace(f'"output": "{new_output}"', f'"output": "{new_output}",\n  "variant-output": "{new_variant}"', 1)
    parsed = json.loads(new_text)  # wirft bei Fehler — dann wird NICHTS geschrieben
    assert parsed["output"] == new_output and parsed["variant-output"] == new_variant
    if not execute:
        return
    backup = cfg_path.with_name(f"{cfg_path.name}.bak-{datetime.now():%Y%m%d-%H%M%S}")
    shutil.copy2(cfg_path, backup)
    tmp = cfg_path.with_suffix(".json.tmp")
    tmp.write_text(new_text, encoding="utf-8")
    json.loads(tmp.read_text(encoding="utf-8"))
    tmp.replace(cfg_path)
    log(f"Config umgestellt (Backup {backup.name})")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--execute", action="store_true", help="scharf ausführen (sonst Probelauf)")
    ap.add_argument("--cut", help="Schnittzeit lokal 'YYYY-MM-DD HH:MM[:SS]' (Default: mtime des Live-Binaries)")
    ap.add_argument("--date", help="Datum für die neuen Analysedateien, DD.MM.YYYY (Default: heute)")
    ap.add_argument("--timeout", type=int, default=30, help="Drain-Wartezeit in Minuten (Default 30)")
    ap.add_argument("--force", action="store_true", help="nicht auf ausstehende Schleusen-Ergebnisse warten")
    ap.add_argument("--keep-done", action="store_true", help="done/ der Schleuse nicht wegräumen")
    ap.add_argument("--schleuse", type=Path, default=DEFAULT_SCHLEUSE)
    a = ap.parse_args()

    cfg = ac.load_config()
    game_dir = (REPO_ROOT / cfg["game-dir"]).resolve()
    archive_dir = game_dir.parent / "game_archiv"
    output_name = str(cfg["output"])
    variant_name = str(cfg.get("variant-output") or ac.default_variant_output(output_name))
    output_path, variant_path = REPO_ROOT / output_name, REPO_ROOT / variant_name
    cut = parse_cut(a.cut)
    date_str = a.date or datetime.now().strftime("%d.%m.%Y")
    if not re.fullmatch(r"\d{2}\.\d{2}\.\d{4}", date_str):
        sys.exit("--date bitte als DD.MM.YYYY")
    new_output = f"analyse-{date_str}.json"
    new_variant = f"analyse-{date_str}-varianten.json"
    if new_output == output_name:
        sys.exit(f"Neue Analysedatei hieße wie die alte ({output_name}) — --date angeben")

    mode = "SCHARF" if a.execute else "PROBELAUF"
    log(f"{mode} — Cut {cut:%Y-%m-%d %H:%M:%S %Z} ({cut.astimezone(timezone.utc):%H:%M:%S} UTC)")
    log(f"game_records: {game_dir}  →  game_archiv: {archive_dir}")
    log(f"Analysedateien: {output_name} / {variant_name}  →  archiv/;  neu: {new_output} / {new_variant}")

    pgns = sorted(game_dir.glob("*.pgn"))
    old = [p for p in pgns if game_start_utc(p) < cut]
    new = [p for p in pgns if p not in set(old)]
    log(f"{len(pgns)} PGNs in game_records: {len(old)} vor dem Cut (archivieren), {len(new)} danach (bleiben)")
    for p in new[:5]:
        log(f"   bleibt: {p.name} (Start {game_start_utc(p):%d.%m. %H:%M} UTC)")

    # --- Drain: alte Partien fertig analysieren lassen -----------------------
    max_fail = int(cfg.get("max-failures", ac.DEFAULT_MAX_FAILURES if hasattr(ac, "DEFAULT_MAX_FAILURES") else 3))
    deadline = time.time() + a.timeout * 60
    while True:
        analyzed = ac.load_analyzed(output_path) | ac.load_analyzed(variant_path)
        quarantined = ac.quarantined_names(ac.load_quarantine(), max_fail)
        pending = [p.name for p in old if p.name not in analyzed and p.name not in quarantined]
        outbox = list((a.schleuse / "outbox").glob("*.pgn")) if (a.schleuse / "outbox").is_dir() else []
        inbox = list((a.schleuse / "inbox").glob("*.json")) if (a.schleuse / "inbox").is_dir() else []
        log(f"alte Partien ohne Ergebnis: {len(pending)}  (outbox {len(outbox)}, inbox {len(inbox)}, quarantäne {len(quarantined)})")
        if not pending or a.force or not a.execute:
            if pending and a.force:
                log(f"--force: {len(pending)} Partien bleiben ohne Analyse (Ergebnisse landen ggf. in der neuen Datei)")
            if pending and not a.execute and not a.force:
                log("(Probelauf: scharf würde hier gewartet, bis die Schleuse diese Partien liefert)")
            break
        if time.time() > deadline:
            sys.exit(f"Drain-Timeout nach {a.timeout} Min — {len(pending)} alte Partien offen. Erneut versuchen oder --force.")
        run_sync()
        time.sleep(60)

    if not a.execute:
        rewrite_config(ac.CONFIG_PATH, new_output, new_variant, execute=False)
        log("Probelauf beendet — nichts verändert. Mit --execute scharf schalten.")
        return 0

    # --- Scharf: Lock halten, verschieben, umstellen -------------------------
    if not ac.acquire_lock():
        sys.exit("Schleuse läuft gerade (Lock) — in einer Minute erneut versuchen.")
    try:
        archive_dir.mkdir(parents=True, exist_ok=True)
        moved = 0
        for p in old:
            shutil.move(str(p), str(unique_target(archive_dir, p.name)))
            moved += 1
        log(f"{moved} PGNs nach {archive_dir} verschoben")

        ARCHIV_DIR.mkdir(exist_ok=True)
        for path in (output_path, variant_path):
            if path.exists():
                target = unique_target(ARCHIV_DIR, path.name)
                shutil.move(str(path), str(target))
                log(f"{path.name} → archiv/{target.name}")
            else:
                log(f"{path.name} existiert nicht (nichts zu archivieren)")

        rewrite_config(ac.CONFIG_PATH, new_output, new_variant, execute=True)

        q = ac.load_quarantine()
        before = len(q)
        ac.save_quarantine(q, game_dir)  # siebt Einträge zu verschobenen PGNs aus
        log(f"Quarantäne: {before} → {len(ac.load_quarantine())} Einträge")

        done = a.schleuse / "done"
        if done.is_dir() and not a.keep_done:
            sub = done / f"bis-{date_str}"
            sub.mkdir(exist_ok=True)
            n = 0
            for f in done.iterdir():
                if f.is_file():
                    shutil.move(str(f), str(unique_target(sub, f.name)))
                    n += 1
            log(f"done/: {n} Dateien nach {sub.name}/ verschoben")
    finally:
        ac.release_lock()

    log("fertig. Kontrolle: `info` (Partien seit letzter Archivierung) und "
        "`tail -n 3 logs/schleuse_sync.log` nach dem nächsten Tick.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
