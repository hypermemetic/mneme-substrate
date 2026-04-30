#!/usr/bin/env python3
"""
Resolution sweeper for the live marketplace pipeline.

Reads programs/_marketwatch/pairings.jsonl, groups by market_id, hits
Manifold's API for each, and:
  - Records resolutions in programs/_marketwatch/resolutions.jsonl
  - Calls forecast.resolve via synapse for each prior forecast on a
    resolved market so the substrate's calibration store grows.
  - Idempotent — skips markets already in resolutions.jsonl.

Usage:
    python3 scripts/marketwatch_resolve.py
    python3 scripts/marketwatch_resolve.py --port 4456
"""
import argparse
import json
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import Request, urlopen

MANIFOLD_API_BASE = "https://api.manifold.markets/v0"
MARKETWATCH_DIR = Path("programs/_marketwatch")
PAIRINGS_PATH = MARKETWATCH_DIR / "pairings.jsonl"
RESOLUTIONS_PATH = MARKETWATCH_DIR / "resolutions.jsonl"


def load_pairings():
    if not PAIRINGS_PATH.exists():
        return []
    out = []
    with PAIRINGS_PATH.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                out.append(json.loads(line))
            except Exception:
                continue
    return out


def already_recorded_market_ids():
    if not RESOLUTIONS_PATH.exists():
        return set()
    out = set()
    with RESOLUTIONS_PATH.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
                if r.get("market_id"):
                    out.add(r["market_id"])
            except Exception:
                continue
    return out


def fetch_market(market_id):
    req = Request(f"{MANIFOLD_API_BASE}/market/{market_id}",
                  headers={"Accept": "application/json"})
    with urlopen(req, timeout=30) as resp:
        return json.load(resp)


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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=4456)
    args = ap.parse_args()

    pairings = load_pairings()
    by_market = {}
    for p in pairings:
        if "error" in p or not p.get("program_id"):
            continue
        by_market.setdefault(p["market_id"], []).append(p)
    print(f"pairings: {len(pairings)} total, {len(by_market)} unique markets")

    already = already_recorded_market_ids()
    print(f"already resolved: {len(already)} markets")

    candidates = [mid for mid in by_market if mid not in already]
    print(f"to check: {len(candidates)}")

    newly_resolved = 0
    still_open = 0
    errors = 0
    MARKETWATCH_DIR.mkdir(parents=True, exist_ok=True)

    for market_id in candidates:
        try:
            m = fetch_market(market_id)
        except Exception as e:
            print(f"  [{market_id[:8]}] fetch error: {e}")
            errors += 1
            continue
        if not m.get("isResolved"):
            still_open += 1
            continue
        resolution = m.get("resolution")
        if resolution not in ("YES", "NO"):
            # MKT or N/A or other resolution — record but don't feed
            # forecast.resolve since we need a clean bool.
            print(f"  [{market_id[:8]}] resolved {resolution} — recording but not feeding calibration")
        resolved_at_unix = m.get("resolutionTime")
        resolved_at_iso = (
            datetime.fromtimestamp(resolved_at_unix / 1000, tz=timezone.utc).isoformat()
            if resolved_at_unix
            else datetime.now(timezone.utc).isoformat()
        )
        actual_bool = resolution == "YES"

        forecasts = by_market[market_id]
        feed_count = 0
        feed_failed = 0
        if resolution in ("YES", "NO"):
            for p in forecasts:
                ok, msg = resolve_in_substrate(
                    p["program_id"], actual_bool, resolved_at_iso, args.port
                )
                if ok:
                    feed_count += 1
                else:
                    feed_failed += 1
                    print(f"    forecast.resolve failed for {p['program_id']}: {msg[:100]}")
                time.sleep(0.1)

        record = {
            "market_id": market_id,
            "ts_recorded": datetime.now(timezone.utc).isoformat(),
            "resolution": resolution,
            "resolved_at": resolved_at_iso,
            "actual_bool": actual_bool if resolution in ("YES", "NO") else None,
            "n_forecasts": len(forecasts),
            "fed_calibration": feed_count,
            "calibration_feed_failed": feed_failed,
            "question": forecasts[0].get("question"),
            "manifold_p_at_first_forecast": forecasts[0].get("manifold_p"),
            "our_p_at_first_forecast": forecasts[0].get("our_p"),
        }
        with RESOLUTIONS_PATH.open("a") as f:
            f.write(json.dumps(record) + "\n")
        newly_resolved += 1
        print(
            f"  [{market_id[:8]}] resolved {resolution} (n={len(forecasts)} forecasts, "
            f"{feed_count} fed to calibration, {feed_failed} failed)"
        )

    print(f"\n--- DONE ---")
    print(f"  newly resolved this pass: {newly_resolved}")
    print(f"  still open: {still_open}")
    print(f"  errors: {errors}")
    print(f"  resolutions file: {RESOLUTIONS_PATH}")


if __name__ == "__main__":
    main()
