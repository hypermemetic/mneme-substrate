#!/usr/bin/env python3
"""
Resolution sweeper for the live marketplace pipeline.

Two-phase model so the substrate can be down without losing data:

  Phase A (network-only): hit Manifold for each unresolved market.
    On resolution, append a row to resolutions.jsonl immediately. No
    substrate calls yet. Safe to run with the substrate down.

  Phase B (substrate-only): scan resolutions.jsonl for rows whose
    calibration feed is still pending/failed, and call
    forecast.resolve for each forecast on that market. Records a
    feed-status row in _calibration_feeds.jsonl per attempt; the
    "most recent status per (market_id, program_id)" wins on retry.
    Idempotent — safe to re-run.

Default --phase=both runs A then B in one process. --phase=a-only
runs Phase A and exits (use when substrate is down). --phase=b-only
runs only the retry sweep.

Files written under programs/_marketwatch/:
    resolutions.jsonl            — append-only, one row per market resolution
    _calibration_feeds.jsonl     — append-only, one row per resolve attempt

Usage:
    python3 scripts/marketwatch_resolve.py
    python3 scripts/marketwatch_resolve.py --port 4456 --phase a-only
    python3 scripts/marketwatch_resolve.py --port 4456 --phase b-only
"""
import argparse
import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

MANIFOLD_API_BASE = "https://api.manifold.markets/v0"
MARKETWATCH_DIR = Path("programs/_marketwatch")
PAIRINGS_PATH = MARKETWATCH_DIR / "pairings.jsonl"
RESOLUTIONS_PATH = MARKETWATCH_DIR / "resolutions.jsonl"
FEEDS_PATH = MARKETWATCH_DIR / "_calibration_feeds.jsonl"


def load_jsonl(path):
    if not path.exists():
        return []
    out = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except Exception:
                continue
    return out


def append_jsonl(path, row):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as f:
        f.write(json.dumps(row) + "\n")


def check_substrate_alive(port: int) -> bool:
    try:
        out = subprocess.run(
            ["synapse", "-P", str(port), "substrate", "hash"],
            capture_output=True, text=True, timeout=5,
        )
        return out.returncode == 0
    except Exception:
        return False


def fetch_market(market_id):
    """Return market dict, or None if Manifold says it's gone (404/410/empty).

    Anything else (transient network errors, 5xx) raises so the caller
    can count it as a soft error and try again next sweep.
    """
    req = Request(f"{MANIFOLD_API_BASE}/market/{market_id}",
                  headers={"Accept": "application/json"})
    try:
        with urlopen(req, timeout=30) as resp:
            body = resp.read()
            if not body:
                return None
            return json.loads(body)
    except HTTPError as e:
        if e.code in (404, 410):
            return None
        raise
    except URLError:
        raise


def resolve_in_substrate(program_id, actual_bool, resolved_at_iso, port):
    params = {
        "program_id": program_id,
        "actual": actual_bool,
        "resolved_at": resolved_at_iso,
    }
    cmd = ["synapse", "-j", "-P", str(port), "-p", json.dumps(params),
           "substrate", "forecast", "resolve"]
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
    if out.returncode != 0:
        return False, out.stderr[:200]
    for line in out.stdout.splitlines():
        try:
            evt = json.loads(line.strip())
            if evt.get("type") == "data" and evt["content"].get("type") == "resolved":
                return True, "resolved"
            if evt.get("type") == "data" and evt["content"].get("type") == "error":
                return False, evt["content"].get("message", "")[:200]
        except Exception:
            continue
    return False, "no resolved event"


def latest_feed_status(feeds, market_id, program_id):
    """Most recent feed-attempt status for (market, forecast). None if untried."""
    matches = [
        f for f in feeds
        if f.get("market_id") == market_id and f.get("program_id") == program_id
    ]
    if not matches:
        return None
    return max(matches, key=lambda f: f.get("ts_unix", 0))


def phase_a(pairings, already_resolved_market_ids):
    """Network-only sweep. Returns counts dict."""
    by_market = {}
    for p in pairings:
        if "error" in p or not p.get("program_id"):
            continue
        by_market.setdefault(p["market_id"], []).append(p)

    candidates = [mid for mid in by_market if mid not in already_resolved_market_ids]
    print(f"phase A: {len(by_market)} unique markets in pairings, "
          f"{len(already_resolved_market_ids)} already resolved, {len(candidates)} to check")

    counts = {"newly_resolved": 0, "still_open": 0, "deleted": 0, "errors": 0}

    for market_id in candidates:
        try:
            m = fetch_market(market_id)
        except Exception as e:
            print(f"  [{market_id[:8]}] fetch error: {e}")
            counts["errors"] += 1
            continue

        if m is None:
            print(f"  [{market_id[:8]}] gone from Manifold (404/empty) — skipping; "
                  f"will retry next sweep")
            counts["deleted"] += 1
            continue

        if not m.get("isResolved"):
            counts["still_open"] += 1
            continue

        resolution = m.get("resolution")
        resolved_at_unix = m.get("resolutionTime")
        resolved_at_iso = (
            datetime.fromtimestamp(resolved_at_unix / 1000, tz=timezone.utc).isoformat()
            if resolved_at_unix
            else datetime.now(timezone.utc).isoformat()
        )
        actual_bool = resolution == "YES" if resolution in ("YES", "NO") else None

        forecasts = by_market[market_id]
        record = {
            "market_id": market_id,
            "ts_recorded": datetime.now(timezone.utc).isoformat(),
            "resolution": resolution,
            "resolved_at": resolved_at_iso,
            "actual_bool": actual_bool,
            "n_forecasts": len(forecasts),
            "fed_calibration": "pending" if resolution in ("YES", "NO") else "n/a",
            "question": forecasts[0].get("question"),
            "manifold_p_at_first_forecast": forecasts[0].get("manifold_p"),
            "our_p_at_first_forecast": forecasts[0].get("our_p"),
        }
        append_jsonl(RESOLUTIONS_PATH, record)
        counts["newly_resolved"] += 1
        suffix = "" if resolution in ("YES", "NO") else "  (won't feed calibration)"
        print(f"  [{market_id[:8]}] resolved {resolution} "
              f"(n={len(forecasts)} forecasts){suffix}")

    return counts


def phase_b(pairings, port):
    """Substrate-side retry of pending/failed calibration feeds."""
    resolutions = load_jsonl(RESOLUTIONS_PATH)
    feeds = load_jsonl(FEEDS_PATH)
    by_market = {}
    for p in pairings:
        if "error" in p or not p.get("program_id"):
            continue
        by_market.setdefault(p["market_id"], []).append(p)

    feedable = [
        r for r in resolutions
        if r.get("resolution") in ("YES", "NO")
        and r.get("fed_calibration") in ("pending", "failed")
    ]
    print(f"phase B: {len(feedable)} resolved markets eligible for calibration feed")

    counts = {"fed": 0, "skipped_done": 0, "failed": 0}

    for r in feedable:
        market_id = r["market_id"]
        actual_bool = r["actual_bool"]
        resolved_at_iso = r["resolved_at"]
        forecasts = by_market.get(market_id, [])
        if not forecasts:
            continue
        for p in forecasts:
            program_id = p["program_id"]
            last = latest_feed_status(feeds, market_id, program_id)
            if last and last.get("status") == "done":
                counts["skipped_done"] += 1
                continue
            ok, msg = resolve_in_substrate(program_id, actual_bool, resolved_at_iso, port)
            now = datetime.now(timezone.utc)
            feed_row = {
                "market_id": market_id,
                "program_id": program_id,
                "ts": now.isoformat(),
                "ts_unix": int(now.timestamp()),
                "status": "done" if ok else "failed",
                "actual_bool": actual_bool,
            }
            if not ok:
                feed_row["error"] = msg[:200]
            append_jsonl(FEEDS_PATH, feed_row)
            feeds.append(feed_row)
            if ok:
                counts["fed"] += 1
                print(f"  [{market_id[:8]}/{program_id[:8]}] fed calibration ✓")
            else:
                counts["failed"] += 1
                print(f"  [{market_id[:8]}/{program_id[:8]}] feed FAILED: {msg[:120]}")
            time.sleep(0.1)

    return counts


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--phase", choices=["both", "a-only", "b-only"], default="both",
                    help="a-only: network sweep only (substrate may be down). "
                         "b-only: retry pending calibration feeds. "
                         "both: phase A then phase B.")
    ap.add_argument("--marketwatch-dir", default=None,
                    help="Override marketwatch state dir (default: programs/_marketwatch). "
                         "Useful for integration tests.")
    args = ap.parse_args()

    if args.marketwatch_dir:
        global MARKETWATCH_DIR, PAIRINGS_PATH, RESOLUTIONS_PATH, FEEDS_PATH
        MARKETWATCH_DIR = Path(args.marketwatch_dir)
        PAIRINGS_PATH = MARKETWATCH_DIR / "pairings.jsonl"
        RESOLUTIONS_PATH = MARKETWATCH_DIR / "resolutions.jsonl"
        FEEDS_PATH = MARKETWATCH_DIR / "_calibration_feeds.jsonl"

    pairings = load_jsonl(PAIRINGS_PATH)
    print(f"pairings: {len(pairings)} total")

    if args.phase in ("b-only", "both"):
        if not check_substrate_alive(args.port):
            print(
                f"ERROR: substrate at port {args.port} not reachable. "
                f"Run with --phase a-only to do the network sweep without substrate, "
                f"or start the substrate with `make run`.",
                file=sys.stderr,
            )
            sys.exit(1)

    a_counts = None
    b_counts = None

    if args.phase in ("a-only", "both"):
        already = {
            r["market_id"] for r in load_jsonl(RESOLUTIONS_PATH)
            if r.get("market_id")
        }
        a_counts = phase_a(pairings, already)

    if args.phase in ("b-only", "both"):
        b_counts = phase_b(pairings, args.port)

    print("\n--- DONE ---")
    if a_counts:
        print(f"  phase A: {a_counts['newly_resolved']} newly resolved, "
              f"{a_counts['still_open']} still open, "
              f"{a_counts['deleted']} gone-from-manifold, "
              f"{a_counts['errors']} fetch errors")
    if b_counts:
        print(f"  phase B: {b_counts['fed']} fed, "
              f"{b_counts['skipped_done']} already done, "
              f"{b_counts['failed']} failed (will retry)")
    print(f"  resolutions: {RESOLUTIONS_PATH}")
    print(f"  calibration feeds log: {FEEDS_PATH}")


if __name__ == "__main__":
    main()
