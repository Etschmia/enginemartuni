#!/usr/bin/env python3
"""Regressionstests für Schleusen-Sync und Archivierung (Code-Review 14.11.2026).

Aufruf:  python3 tools/test_schleuse_tools.py
Arbeitet ausschließlich in temporären Verzeichnissen.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import analyze_cron as ac  # noqa: E402
import schleuse_sync as ss  # noqa: E402


class ImportOrderTest(unittest.TestCase):
    """Befund 3: Ergebnisse erst bestätigen, wenn der Zustand gespeichert ist."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.inbox, self.outbox, self.done, self.games = (
            root / "inbox", root / "outbox", root / "done", root / "games",
        )
        for d in (self.inbox, self.outbox, self.done, self.games):
            d.mkdir()
        self.output = root / "analyse.json"
        self.variant_output = root / "analyse-varianten.json"
        result = {
            "analyzed_pgns": ["a.pgn"],
            "blunders": [{"game_id": "a", "ply": 12, "variant": "standard"}],
        }
        (self.inbox / "a.pgn.json").write_text(json.dumps(result), encoding="utf-8")
        (self.outbox / "a.pgn").write_text('[Variant "Standard"]\n\n1. e4 *\n', encoding="utf-8")
        (self.games / "a.pgn").write_text('[Variant "Standard"]\n\n1. e4 *\n', encoding="utf-8")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_import(self) -> tuple[int, int]:
        return ss.import_results(
            self.inbox, self.outbox, self.done, self.games,
            self.output, self.variant_output, {}, 3, False,
        )

    def test_failed_save_keeps_result_in_inbox(self) -> None:
        with mock.patch.object(ss, "save_state", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                self.run_import()
        self.assertTrue((self.inbox / "a.pgn.json").exists(), "Ergebnis bleibt in inbox/")
        self.assertFalse((self.done / "a.pgn.json").exists(), "nichts in done/ bestätigt")
        self.assertTrue((self.outbox / "a.pgn").exists(), "Outbox-Kopie bleibt")

        # Nächster Tick ohne Fehler holt das Ergebnis nach.
        self.assertEqual(self.run_import(), (1, 0))
        state = json.loads(self.output.read_text(encoding="utf-8"))
        self.assertEqual(state["analyzed_pgns"], ["a.pgn"])
        self.assertEqual(len(state["blunders"]), 1)
        self.assertTrue((self.done / "a.pgn.json").exists())
        self.assertFalse((self.outbox / "a.pgn").exists())

    def test_reimport_after_crash_between_save_and_confirm(self) -> None:
        """Gespeichert, aber nicht mehr verschoben → erneuter Import ohne Doppelte."""
        with mock.patch.object(ss, "move_to_done", side_effect=OSError("crash")):
            with self.assertRaises(OSError):
                self.run_import()
        self.assertTrue(self.output.exists())
        self.assertEqual(self.run_import(), (1, 0))
        state = json.loads(self.output.read_text(encoding="utf-8"))
        self.assertEqual(len(state["blunders"]), 1, "Blunder nicht verdoppelt")
        self.assertEqual(state["analyzed_pgns"], ["a.pgn"])


class SharedLockTest(unittest.TestCase):
    """Befund 4: Archivierung und Sync schließen sich gegenseitig aus."""

    def setUp(self) -> None:
        self.saved = ac.LOCK_PATH

    def tearDown(self) -> None:
        ac.LOCK_PATH = self.saved

    def test_archive_uses_the_sync_lock(self) -> None:
        import archive_after_rollout as ar

        self.assertEqual(ar.SYNC_LOCK_PATH, ss.LOCK_PATH)

    def test_lock_held_by_live_process_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ac.LOCK_PATH = Path(tmp) / "schleuse_sync.pid"
            # Der "andere" Prozess: PID 1 lebt immer.
            ac.LOCK_PATH.write_text("1")
            self.assertFalse(ac.acquire_lock())
            self.assertEqual(ac.LOCK_PATH.read_text(), "1", "fremder Lock unangetastet")

    def test_stale_lock_is_replaced_and_released(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ac.LOCK_PATH = Path(tmp) / "schleuse_sync.pid"
            ac.LOCK_PATH.write_text("999999999")  # kein solcher Prozess
            self.assertTrue(ac.acquire_lock())
            self.assertEqual(ac.LOCK_PATH.read_text(), str(os.getpid()))
            # Zweiter Versuch (derselbe lebende Prozess hält ihn) scheitert.
            self.assertFalse(ac.acquire_lock())
            ac.release_lock()
            self.assertFalse(ac.LOCK_PATH.exists())


if __name__ == "__main__":
    unittest.main()
