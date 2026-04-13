#!/usr/bin/env python3
"""Cron job: find an online Lichess bot, challenge it, track results.

Modes:
  rapid  — runs at X:45, alternates between 10+5 and 15+10
  blitz  — runs at X:30, alternates between 3+0 and 5+0
"""

import argparse
import json
import os
import random
import signal
import sys
import time
import logging
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urljoin

import requests
import yaml

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
_parser = argparse.ArgumentParser()
_parser.add_argument(
    "--mode",
    choices=["rapid", "blitz"],
    default="rapid",
    help="Time-control mode: 'rapid' (10+5/15+10) or 'blitz' (3+0/5+0)",
)
args = _parser.parse_args()
MODE = args.mode

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
SCRIPT_DIR = Path(__file__).resolve().parent
CONFIG_PATH = SCRIPT_DIR / "config.yml"
STATE_FILE = SCRIPT_DIR / f"challenge_cron_state_{MODE}.json"
TRACKING_FILE = SCRIPT_DIR / "challenge_cron_tracking.json"
LOG_FILE = SCRIPT_DIR / "lichess_bot_auto_logs" / f"challenge_cron_{MODE}.log"

# ---------------------------------------------------------------------------
# Mode-specific constants
# ---------------------------------------------------------------------------
if MODE == "rapid":
    # Runs at X:45 — up to ~55 min until next run
    CHALLENGE_WAIT = 55 * 60
    TC_OPTIONS = [
        (600, 5, "10+5"),
        (900, 10, "15+10"),
    ]
else:  # blitz
    # Runs at X:30 — up to ~25 min until next run
    CHALLENGE_WAIT = 25 * 60
    TC_OPTIONS = [
        (180, 0, "3+0"),
        (300, 0, "5+0"),
    ]

POLL_INTERVAL = 5  # check every 5 seconds

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------
LOG_FILE.parent.mkdir(parents=True, exist_ok=True)
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s  %(levelname)-8s  %(message)s",
    handlers=[
        logging.FileHandler(LOG_FILE),
        logging.StreamHandler(),
    ],
)
log = logging.getLogger(f"challenge_cron.{MODE}")

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
with open(CONFIG_PATH) as f:
    cfg = yaml.safe_load(f)

TOKEN = cfg["token"]
BASE_URL = cfg.get("url", "https://lichess.org/")
HEADERS = {"Authorization": f"Bearer {TOKEN}"}
TIMEOUT = 10  # HTTP timeout in seconds


# ---------------------------------------------------------------------------
# Helpers: Lichess API
# ---------------------------------------------------------------------------
def api_get(path: str, extra_headers: dict | None = None, **kwargs) -> requests.Response:
    url = urljoin(BASE_URL, path)
    h = {**HEADERS, **(extra_headers or {})}
    return requests.get(url, headers=h, timeout=TIMEOUT, **kwargs)


def api_post(path: str, **kwargs) -> requests.Response:
    url = urljoin(BASE_URL, path)
    return requests.post(url, headers=HEADERS, timeout=TIMEOUT, **kwargs)


def get_my_username() -> str:
    r = api_get("/api/account")
    r.raise_for_status()
    return r.json()["username"]


def get_online_bots() -> list[dict]:
    """Return list of online bot profiles (NDJSON)."""
    r = api_get("/api/bot/online", stream=False)
    r.raise_for_status()
    bots = []
    for line in r.text.strip().split("\n"):
        line = line.strip()
        if line:
            bots.append(json.loads(line))
    return bots


def get_my_ongoing_games() -> list[dict]:
    """Return list of the bot's currently ongoing games."""
    r = api_get("/api/account/playing")
    r.raise_for_status()
    return r.json().get("nowPlaying", [])


def create_challenge(opponent: str, clock_limit: int, clock_increment: int) -> dict:
    """Send a challenge.  Returns the API response JSON."""
    payload = {
        "rated": "true",
        "clock.limit": str(clock_limit),
        "clock.increment": str(clock_increment),
        "variant": "standard",
    }
    r = api_post(f"/api/challenge/{opponent}", data=payload)
    return r.json()


def cancel_challenge(challenge_id: str) -> None:
    api_post(f"/api/challenge/{challenge_id}/cancel")


def check_challenge_outcome(challenge_id: str) -> str:
    """Infer the challenge outcome without needing challenge:read scope.

    Uses the public game export: if the challenge was accepted, a game with the
    same ID exists.  Returns:
      "game_live"    — game exists and is still ongoing
      "game_done"    — game exists and is finished
      "no_game"      — no game found (challenge may still be pending, or was declined)
    """
    try:
        r = api_get(f"/game/export/{challenge_id}", extra_headers={"Accept": "application/json"})
        if r.status_code == 404:
            return "no_game"
        r.raise_for_status()
        data = r.json()
        status = data.get("status", "")
        if status in ("started", "created"):
            return "game_live"
        return "game_done"
    except Exception:
        return "no_game"


def get_game_status(game_id: str) -> dict:
    """Export a finished (or ongoing) game."""
    r = api_get(f"/game/export/{game_id}", extra_headers={"Accept": "application/json"})
    if r.status_code == 404:
        return {}
    r.raise_for_status()
    return r.json()


# ---------------------------------------------------------------------------
# Tracking persistence
# ---------------------------------------------------------------------------
def load_tracking() -> dict:
    if TRACKING_FILE.exists():
        with open(TRACKING_FILE) as f:
            return json.load(f)
    return {"no_response": [], "accepted": [], "accepted_then_left": [], "no_bot_challenges": []}


def save_tracking(data: dict) -> None:
    with open(TRACKING_FILE, "w") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)


def record(category: str, opponent: str, time_control: str) -> None:
    """Append an entry to the tracking file."""
    tracking = load_tracking()
    entry = {
        "bot": opponent,
        "time_control": time_control,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }
    tracking.setdefault(category, []).append(entry)
    save_tracking(tracking)
    log.info("Tracked %s: %s (%s)", category, opponent, time_control)


# ---------------------------------------------------------------------------
# State / PID management
# ---------------------------------------------------------------------------
def write_state(state: str, pid: int | None = None, extra: dict | None = None) -> None:
    obj = {"state": state, "pid": pid or os.getpid(), "ts": datetime.now(timezone.utc).isoformat()}
    if extra:
        obj.update(extra)
    with open(STATE_FILE, "w") as f:
        json.dump(obj, f)


def read_state() -> dict | None:
    if not STATE_FILE.exists():
        return None
    try:
        with open(STATE_FILE) as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError):
        return None


def pid_is_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def handle_previous_instance() -> bool:
    """Handle the previous cron instance of the same mode.

    Returns True if we should continue, False if we should exit.
    """
    prev = read_state()
    if prev is None:
        log.info("No previous instance state found — proceeding.")
        return True

    prev_pid = prev.get("pid", 0)
    prev_state = prev.get("state", "done")

    if not pid_is_alive(prev_pid):
        log.info("Previous instance (PID %d, state=%s) is no longer running — proceeding.", prev_pid, prev_state)
        return True

    if prev_state == "playing":
        log.info("Previous instance (PID %d) is still PLAYING — exiting to not interfere.", prev_pid)
        return False

    if prev_state == "waiting":
        log.info("Previous instance (PID %d) is still WAITING — terminating it.", prev_pid)
        try:
            os.kill(prev_pid, signal.SIGTERM)
            for _ in range(10):
                if not pid_is_alive(prev_pid):
                    break
                time.sleep(1)
            else:
                log.warning("Previous instance did not exit after SIGTERM, sending SIGKILL.")
                os.kill(prev_pid, signal.SIGKILL)
        except OSError as e:
            log.warning("Could not kill previous instance PID %d: %s", prev_pid, e)
        return True

    # Any other state (e.g. "done") — previous finished normally
    log.info("Previous instance finished normally (state=%s) — proceeding.", prev_state)
    return True


# ---------------------------------------------------------------------------
# Choose time control (mode-aware, alternates within mode's TC_OPTIONS)
# ---------------------------------------------------------------------------
def choose_time_control() -> tuple[int, int, str]:
    """Return (clock_limit_seconds, clock_increment_seconds, label).

    Alternates between TC_OPTIONS[0] and TC_OPTIONS[1] based on the last
    accepted game that used one of this mode's time controls.
    """
    tc_labels = {tc[2] for tc in TC_OPTIONS}
    tracking = load_tracking()
    accepted = tracking.get("accepted", [])

    last_tc = None
    for entry in reversed(accepted):
        if entry.get("time_control", "") in tc_labels:
            last_tc = entry["time_control"]
            break

    if last_tc == TC_OPTIONS[0][2]:
        return TC_OPTIONS[1]
    else:
        return TC_OPTIONS[0]


# ---------------------------------------------------------------------------
# Choose opponent
# ---------------------------------------------------------------------------
def choose_opponent(my_username: str, exclude: set[str] | None = None) -> dict | None:
    """Pick a random online bot that accepts bot challenges."""
    exclude = exclude or set()
    tracking = load_tracking()

    # Bots that never responded (accumulated over time — deprioritize but don't fully block)
    no_response_bots = {e["bot"].lower() for e in tracking.get("no_response", [])}
    # Bots that left games — block them permanently
    left_bots = {e["bot"].lower() for e in tracking.get("accepted_then_left", [])}
    # Bots that don't accept bot challenges — block them permanently
    nobot_bots = {e["bot"].lower() for e in tracking.get("no_bot_challenges", [])}

    online = get_online_bots()
    candidates = []
    for bot in online:
        username = bot.get("username", "")
        if username.lower() == my_username.lower():
            continue
        if username.lower() in left_bots:
            continue
        if username.lower() in nobot_bots:
            continue
        if username.lower() in exclude:
            continue
        candidates.append(bot)

    if not candidates:
        log.warning("No eligible online bots found.")
        return None

    # Prefer bots we haven't challenged yet (or that responded before)
    fresh = [b for b in candidates if b["username"].lower() not in no_response_bots]
    if fresh:
        return random.choice(fresh)
    return random.choice(candidates)


# ---------------------------------------------------------------------------
# Main flow
# ---------------------------------------------------------------------------
def main() -> None:
    log.info("=" * 60)
    log.info("Challenge cron started (mode=%s, PID %d)", MODE, os.getpid())

    # Step 1: handle previous instance of the same mode
    if not handle_previous_instance():
        sys.exit(0)

    # Step 2: check that no games are currently ongoing
    ongoing = get_my_ongoing_games()
    if ongoing:
        log.info("Bot has %d ongoing game(s) — exiting without challenging.", len(ongoing))
        write_state("done")
        sys.exit(0)

    # Step 3: choose opponent and time control
    my_username = get_my_username()
    log.info("Bot account: %s", my_username)

    clock_limit, clock_inc, tc_label = choose_time_control()
    log.info("Selected time control: %s", tc_label)

    # Try multiple opponents — some may decline immediately or after a short wait
    MAX_ATTEMPTS = 10
    tried_this_run: set[str] = set()

    for attempt in range(1, MAX_ATTEMPTS + 1):
        opponent_info = choose_opponent(my_username, exclude=tried_this_run)
        if opponent_info is None:
            log.warning("No more eligible bots to challenge.")
            write_state("done")
            sys.exit(0)

        opponent = opponent_info["username"]
        tried_this_run.add(opponent.lower())
        log.info("Attempt %d: challenging %s to a %s game", attempt, opponent, tc_label)

        write_state("waiting", extra={"opponent": opponent, "time_control": tc_label})
        resp = create_challenge(opponent, clock_limit, clock_inc)

        # Check for immediate decline in the response
        decline_key = resp.get("declineReasonKey", "")
        if decline_key:
            log.info("Challenge to %s immediately declined: %s (%s)",
                     opponent, resp.get("declineReason", ""), decline_key)
            if decline_key.lower() == "nobot":
                record("no_bot_challenges", opponent, tc_label)
            else:
                record("no_response", opponent, tc_label)
            continue  # try next bot

        # The API returns the challenge either at top level or nested under "challenge"
        challenge_data = resp.get("challenge", resp)
        if "id" not in challenge_data:
            error_msg = resp.get("error", str(resp))
            log.error("Challenge creation failed: %s", error_msg)
            continue  # try next bot

        challenge_id = challenge_data["id"]
        log.info("Challenge created: %s (id=%s)", opponent, challenge_id)

        # Poll until accepted, declined, or timeout
        result = wait_for_challenge(challenge_id, opponent, tc_label, my_username)

        if result == "accepted":
            return  # game handled inside wait_for_challenge
        elif result == "declined":
            continue  # try next bot
        elif result == "timeout":
            write_state("done")
            sys.exit(0)

    log.warning("Exhausted %d attempts — no bot accepted.", MAX_ATTEMPTS)
    write_state("done")
    sys.exit(0)


def wait_for_challenge(challenge_id: str, opponent: str, tc_label: str, my_username: str) -> str:
    """Wait for a challenge to be accepted, declined, or timeout.

    Returns: "accepted", "declined", or "timeout".
    """
    start_time = time.monotonic()

    while time.monotonic() - start_time < CHALLENGE_WAIT:
        time.sleep(POLL_INTERVAL)

        # Check if the challenge turned into a game via nowPlaying
        ongoing = get_my_ongoing_games()
        our_game = None
        for g in ongoing:
            opp = g.get("opponent", {}).get("username", "")
            opp_id = g.get("opponent", {}).get("id", "")
            if opp.lower() == opponent.lower() or opp_id.lower() == opponent.lower() \
               or opp.lower().removeprefix("bot ") == opponent.lower():
                our_game = g
                break

        if our_game:
            game_id = our_game.get("gameId", our_game.get("id", ""))
            log.info("Challenge accepted! Game %s started vs %s", game_id, opponent)
            record("accepted", opponent, tc_label)
            write_state("playing", extra={"opponent": opponent, "game_id": game_id, "time_control": tc_label})
            monitor_game(game_id, opponent, tc_label, my_username)
            return "accepted"

        # Check if the challenge led to a game (works without challenge:read scope)
        outcome = check_challenge_outcome(challenge_id)
        if outcome == "game_done":
            log.info("Challenge accepted and game already finished (id=%s)", challenge_id)
            record("accepted", opponent, tc_label)
            game_data = get_game_status(challenge_id)
            status = game_data.get("status", "unknown")
            log.info("Game result: status=%s, winner=%s", status, game_data.get("winner", ""))
            if status == "noStart":
                record("accepted_then_left", opponent, tc_label)
            write_state("done")
            log.info("Challenge cron finished.")
            sys.exit(0)
        elif outcome == "game_live":
            log.info("Challenge accepted! Game %s vs %s is live", challenge_id, opponent)
            record("accepted", opponent, tc_label)
            write_state("playing", extra={"opponent": opponent, "game_id": challenge_id, "time_control": tc_label})
            monitor_game(challenge_id, opponent, tc_label, my_username)
            return "accepted"

        # No game exists — check if challenge was declined (gone from Lichess)
        # Wait at least 30s before checking, to give the opponent time to see it
        if time.monotonic() - start_time > 30 and not ongoing:
            try:
                cr = api_post(f"/api/challenge/{challenge_id}/cancel")
                if cr.status_code == 404:
                    log.info("Challenge %s to %s was declined (challenge gone).",
                             challenge_id, opponent)
                    record("no_response", opponent, tc_label)
                    return "declined"
            except Exception:
                pass

        elapsed = int(time.monotonic() - start_time)
        if elapsed % 300 < POLL_INTERVAL:
            log.info("Still waiting for %s to accept (%d min elapsed)...", opponent, elapsed // 60)

    log.info("Challenge to %s timed out after %d minutes — cancelling.",
             opponent, CHALLENGE_WAIT // 60)
    cancel_challenge(challenge_id)
    record("no_response", opponent, tc_label)
    return "timeout"


def monitor_game(game_id: str, opponent: str, tc_label: str, my_username: str) -> None:
    """Wait for a game to finish, then log the result."""
    log.info("Monitoring game %s until it finishes...", game_id)

    while True:
        time.sleep(POLL_INTERVAL)
        ongoing = get_my_ongoing_games()
        still_playing = any(
            (g.get("gameId", g.get("id", "")) == game_id) for g in ongoing
        )
        if not still_playing:
            break

    log.info("Game %s finished. Checking result...", game_id)
    game_data = get_game_status(game_id)
    status = game_data.get("status", "unknown")
    winner = game_data.get("winner", "")

    log.info("Game result: status=%s, winner=%s", status, winner)

    if status == "noStart":
        record("accepted_then_left", opponent, tc_label)
        log.info("Opponent %s accepted but never started the game.", opponent)

    write_state("done")
    log.info("Challenge cron finished.")


# ---------------------------------------------------------------------------
# Graceful shutdown on SIGTERM (sent by next cron instance)
# ---------------------------------------------------------------------------
def sigterm_handler(signum, frame):
    log.info("Received SIGTERM — shutting down gracefully.")
    state = read_state()
    if state and state.get("state") == "waiting":
        cid = state.get("challenge_id")
        if cid:
            try:
                cancel_challenge(cid)
            except Exception:
                pass
    write_state("done")
    sys.exit(0)


signal.signal(signal.SIGTERM, sigterm_handler)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        log.exception("Unhandled exception in challenge cron")
        write_state("done")
        sys.exit(1)
