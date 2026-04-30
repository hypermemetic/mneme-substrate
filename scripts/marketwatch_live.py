#!/usr/bin/env python3
"""
Live marketplace pipeline — single-pass forecaster against Manifold.

Operation:
  1. Fetches open binary markets from Manifold's public API
     (https://api.manifold.markets/v0/markets) matching:
       - outcomeType == "BINARY"
       - isResolved == false
       - closeTime within --close-min..--close-max days from now
       - volume >= --min-volume
  2. For each selected market, checks
     `programs/_marketwatch/pairings.jsonl` to decide whether we've
     already forecasted recently:
       - First pass: forecast it.
       - Subsequent: forecast only if |current_p - last_p| >
         --price-delta-threshold OR
         time_since_last > --time-threshold-hours.
  3. Fires `forecast.update` via synapse for each market that needs a
     fresh forecast.
  4. Appends one row per fresh forecast to pairings.jsonl with both
     manifold's crowd p and mneme's our_p (post-Platt) and our_raw_p
     (pre-Platt).

Designed for cron: re-runs are idempotent within the thresholds.

Usage:
    python3 scripts/marketwatch_live.py
    python3 scripts/marketwatch_live.py --max-markets 20 --min-volume 200 \\
        --close-min 14 --close-max 60 --port 4456 \\
        --price-delta-threshold 0.05 --time-threshold-hours 24

State files written under programs/_marketwatch/:
    pairings.jsonl     — append-only log of all forecasts vs market price
    selected.json      — last selected market list for diagnostic / debug
"""
import argparse
import json
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timedelta, timezone
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.parse import urlencode

MANIFOLD_API_BASE = "https://api.manifold.markets/v0"
MARKETWATCH_DIR = Path("programs/_marketwatch")
PAIRINGS_PATH = MARKETWATCH_DIR / "pairings.jsonl"
SELECTED_PATH = MARKETWATCH_DIR / "selected.json"


def fetch_open_binary_markets(max_pages=10, page_size=100):
    """Paginate through Manifold's recent markets, filter to open binary."""
    out = []
    cursor = None
    for _ in range(max_pages):
        params = {"limit": page_size}
        if cursor:
            params["before"] = cursor
        url = f"{MANIFOLD_API_BASE}/markets?{urlencode(params)}"
        req = Request(url, headers={"Accept": "application/json"})
        with urlopen(req, timeout=30) as resp:
            page = json.load(resp)
        if not page:
            break
        for m in page:
            if m.get("outcomeType") == "BINARY" and not m.get("isResolved", True):
                out.append(m)
        cursor = page[-1].get("id")
        if cursor is None:
            break
    return out


def select_markets(markets, *, close_min_days, close_max_days, min_volume, max_markets):
    """Filter + rank markets for forecasting."""
    now = datetime.now(timezone.utc)
    lower = now + timedelta(days=close_min_days)
    upper = now + timedelta(days=close_max_days)
    filtered = []
    for m in markets:
        if m.get("volume", 0) < min_volume:
            continue
        close_ts = m.get("closeTime")
        if close_ts is None:
            continue
        close_dt = datetime.fromtimestamp(close_ts / 1000, tz=timezone.utc)
        if not (lower <= close_dt <= upper):
            continue
        filtered.append((m, close_dt))
    # Rank by volume (proxy for "this market is being taken seriously")
    filtered.sort(key=lambda mc: -mc[0].get("volume", 0))
    return filtered[:max_markets]


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


def needs_forecast(market_id, current_p, prior_pairings, *, price_delta, time_threshold_hours):
    """Returns True if we should fire a fresh forecast for this market."""
    relevant = [p for p in prior_pairings if p.get("market_id") == market_id]
    if not relevant:
        return True, "first forecast"
    last = max(relevant, key=lambda p: p.get("ts_unix", 0))
    last_p = last.get("manifold_p")
    last_ts_unix = last.get("ts_unix", 0)
    now_unix = int(time.time())
    if abs(current_p - last_p) >= price_delta:
        return True, f"price moved ({last_p:.3f} → {current_p:.3f})"
    if (now_unix - last_ts_unix) >= time_threshold_hours * 3600:
        hours = (now_unix - last_ts_unix) / 3600
        return True, f"stale ({hours:.0f}h since last)"
    return False, "no refresh needed"


def fire_forecast(market, port, trials, iterative):
    """Fire one forecast.update against the substrate."""
    new_evidence = (
        f"Question: {market['question']}\n\n"
        f"Description: {market.get('description') or '(none)'}\n\n"
        f"Source: manifold-live\n"
        f"Market URL: {market.get('url')}\n"
        f"Current crowd probability (manifold): {market.get('probability', 0.5):.4f}\n"
        f"Resolution criteria: This market resolves on Manifold according to "
        f"its own resolution criteria; we'll observe the eventual binary outcome.\n\n"
        f"Forecast the probability the market resolves YES."
    )
    params = {
        "program_id": f"MANIFOLD-LIVE-{market['id']}",
        "new_evidence": new_evidence,
        "trials": trials,
        "allowed_tools": ["WebSearch"],
    }
    if iterative > 0:
        params["iterative_max_steps"] = iterative
    cmd = [
        "synapse", "-j", "-P", str(port), "-p", json.dumps(params),
        "substrate", "forecast", "update",
    ]
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    if out.returncode != 0:
        return {"error": f"synapse: {out.stderr[:200]}"}
    program_id = None
    for line in out.stdout.splitlines():
        try:
            evt = json.loads(line.strip())
            if evt.get("type") == "data" and evt["content"].get("type") == "started":
                program_id = evt["content"]["program_id"]
                break
        except Exception:
            continue
    if not program_id:
        return {"error": f"no program_id in synapse output"}
    artifact = Path("programs") / program_id / "artifact.json"
    error_path = Path("programs") / program_id / "error.json"
    deadline = time.time() + 720
    while time.time() < deadline:
        if artifact.exists():
            try:
                a = json.loads(artifact.read_text())
                return {
                    "program_id": program_id,
                    "our_p": a.get("probability"),
                    "our_raw_p": a.get("raw_probability"),
                    "summary": a.get("summary", "")[:500],
                    "n_trials": a.get("n_trials"),
                }
            except Exception as e:
                return {"error": f"artifact parse: {e}", "program_id": program_id}
        if error_path.exists():
            return {"error": f"program error: {error_path.read_text()[:200]}",
                    "program_id": program_id}
        time.sleep(3)
    return {"error": "polling timeout (12 min)", "program_id": program_id}


def append_pairing(market, close_dt, forecast_result):
    MARKETWATCH_DIR.mkdir(parents=True, exist_ok=True)
    now = datetime.now(timezone.utc)
    row = {
        "ts": now.isoformat(),
        "ts_unix": int(now.timestamp()),
        "market_id": market["id"],
        "market_url": market.get("url"),
        "question": market["question"],
        "close_time": close_dt.isoformat(),
        "manifold_p": market.get("probability", 0.5),
        "volume": market.get("volume", 0),
    }
    if "error" in forecast_result:
        row["error"] = forecast_result["error"]
        row["program_id"] = forecast_result.get("program_id")
    else:
        row.update(
            our_p=forecast_result["our_p"],
            our_raw_p=forecast_result.get("our_raw_p"),
            program_id=forecast_result["program_id"],
            n_trials=forecast_result.get("n_trials"),
            summary=forecast_result.get("summary", ""),
        )
    with PAIRINGS_PATH.open("a") as f:
        f.write(json.dumps(row) + "\n")
    return row


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--max-markets", type=int, default=10)
    ap.add_argument("--min-volume", type=float, default=100.0)
    ap.add_argument("--close-min", type=int, default=14, help="min days until close")
    ap.add_argument("--close-max", type=int, default=60, help="max days until close")
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--trials", type=int, default=2)
    ap.add_argument("--iterative-max-steps", type=int, default=5)
    ap.add_argument("--concurrency", type=int, default=4)
    ap.add_argument("--price-delta-threshold", type=float, default=0.05,
                    help="re-forecast if |current - last| >= this")
    ap.add_argument("--time-threshold-hours", type=float, default=24,
                    help="re-forecast if last forecast is older than this")
    ap.add_argument("--max-pages", type=int, default=10)
    args = ap.parse_args()

    print(f"fetching open binary markets from manifold (max {args.max_pages} pages)...", flush=True)
    candidates = fetch_open_binary_markets(max_pages=args.max_pages)
    print(f"  got {len(candidates)} open binary markets", flush=True)

    selected = select_markets(
        candidates,
        close_min_days=args.close_min,
        close_max_days=args.close_max,
        min_volume=args.min_volume,
        max_markets=args.max_markets,
    )
    print(f"  selected {len(selected)} after filter "
          f"(close {args.close_min}-{args.close_max}d, vol>={args.min_volume})",
          flush=True)

    SELECTED_PATH.parent.mkdir(parents=True, exist_ok=True)
    SELECTED_PATH.write_text(json.dumps(
        [{"id": m["id"], "question": m["question"], "manifold_p": m.get("probability", 0.5),
          "volume": m.get("volume", 0), "close_time": dt.isoformat()}
         for m, dt in selected],
        indent=2,
    ))

    prior = load_pairings()
    needs = []
    for m, dt in selected:
        ok, why = needs_forecast(
            m["id"], m.get("probability", 0.5), prior,
            price_delta=args.price_delta_threshold,
            time_threshold_hours=args.time_threshold_hours,
        )
        if ok:
            needs.append((m, dt, why))
        else:
            print(f"  [{m['id'][:8]}] skip: {why}", flush=True)

    if not needs:
        print("nothing to forecast — all watched markets are within thresholds")
        return

    print(f"firing {len(needs)} forecasts at concurrency {args.concurrency}", flush=True)
    t0 = time.time()
    rows = []
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futs = {
            pool.submit(fire_forecast, m, args.port, args.trials, args.iterative_max_steps): (m, dt, why)
            for m, dt, why in needs
        }
        for fut in as_completed(futs):
            m, dt, why = futs[fut]
            res = fut.result()
            row = append_pairing(m, dt, res)
            rows.append(row)
            if "error" in res:
                print(f"  [{m['id'][:8]}] FAIL ({why}): {res['error'][:120]}", flush=True)
            else:
                delta = res["our_p"] - m.get("probability", 0.5)
                print(
                    f"  [{m['id'][:8]}] manifold={m.get('probability', 0.5):.3f} "
                    f"mneme={res['our_p']:.3f} (raw={res.get('our_raw_p', 0):.3f}) "
                    f"Δ={delta:+.3f}  ({why})",
                    flush=True,
                )
    elapsed = time.time() - t0
    succ = [r for r in rows if "error" not in r]
    fail = [r for r in rows if "error" in r]
    print(f"\n--- DONE in {elapsed:.0f}s ---")
    print(f"  forecasts: {len(succ)} / {len(needs)} succeeded")
    if fail:
        print(f"  {len(fail)} failed")
    if succ:
        deltas = [r["our_p"] - r["manifold_p"] for r in succ]
        avg = sum(deltas) / len(deltas)
        print(f"  mean (our_p - manifold_p) on this pass: {avg:+.4f}")
    print(f"\npairings appended to {PAIRINGS_PATH}")


if __name__ == "__main__":
    main()
