#!/usr/bin/env python3
"""
Resolve bench-003 predictions into the substrate's calibration store.

Reads programs/_benchmarks/runs/<bench-003-dir>/results.jsonl, finds each
prediction's program_id + the question's resolved_to value, and calls
substrate.forecast.resolve so the calibration store grows.

Idempotent — skips program_ids already present in the calibration store.

Usage:
    python3 scripts/resolve_bench_003.py
    python3 scripts/resolve_bench_003.py --port 4456 \\
        --results programs/_benchmarks/runs/<dir>/results.jsonl \\
        --calibration-history programs/_calibration/history.jsonl
"""
import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

DEFAULT_RESULTS = "programs/_benchmarks/runs/20260426-165255-n20-k2/results.jsonl"
DEFAULT_HISTORY = "programs/_calibration/history.jsonl"
DEFAULT_BIAS = "programs/_calibration/bias.json"


def load_records(path: Path):
    """Load bench results, filter to records with usable program_id + actual."""
    out = []
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except Exception as e:
                print(f"WARN: bad json line: {e}", file=sys.stderr)
                continue
            if "error" in r:
                continue
            pid = r.get("program_id")
            actual_f = r.get("actual")
            if pid is None or actual_f is None:
                continue
            out.append(r)
    return out


def already_resolved(history_path: Path):
    """Return set of program_ids already in the calibration store."""
    if not history_path.exists():
        return set()
    out = set()
    with history_path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obs = json.loads(line)
                if "program_id" in obs:
                    out.add(obs["program_id"])
            except Exception:
                continue
    return out


def resolve_one(port: int, program_id: str, actual: bool, resolved_at: str):
    """Call substrate.forecast.resolve via synapse."""
    params = {
        "program_id": program_id,
        "actual": actual,
        "resolved_at": resolved_at,
    }
    cmd = [
        "synapse", "-j", "-P", str(port), "-p", json.dumps(params),
        "substrate", "forecast", "resolve",
    ]
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
    if out.returncode != 0:
        return False, f"synapse exit {out.returncode}: {out.stderr[:200]}"
    # Look for a "resolved" event
    for line in out.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            evt = json.loads(line)
        except Exception:
            continue
        if not isinstance(evt, dict):
            continue
        content = evt.get("content")
        if isinstance(content, dict) and content.get("type") == "resolved":
            return True, "resolved"
        if isinstance(content, dict) and content.get("type") == "error":
            return False, f"server error: {content.get('message', '')[:200]}"
    return False, f"no resolved event in synapse output: {out.stdout[:300]}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--results", default=DEFAULT_RESULTS)
    ap.add_argument("--calibration-history", default=DEFAULT_HISTORY)
    ap.add_argument("--bias", default=DEFAULT_BIAS)
    args = ap.parse_args()

    results_path = Path(args.results)
    history_path = Path(args.calibration_history)
    bias_path = Path(args.bias)

    if not results_path.exists():
        print(f"ERROR: {results_path} not found", file=sys.stderr)
        sys.exit(1)

    records = load_records(results_path)
    seen = already_resolved(history_path)
    print(f"loaded {len(records)} usable records from {results_path}")
    print(f"calibration store has {len(seen)} program_ids already")

    added = 0
    skipped_already = 0
    skipped_fractional = 0
    failed = []

    for r in records:
        pid = r["program_id"]
        actual_f = float(r["actual"])
        if pid in seen:
            skipped_already += 1
            continue
        # Skip fractional resolutions — substrate's ResolvedObservation
        # currently takes actual: bool. Threshold at 0.5 OR skip if neither
        # 0.0 nor 1.0. Conservative: skip strictly fractional values
        # because the underlying market resolved ambiguously and our
        # current schema can't represent that without lying.
        if actual_f not in (0.0, 1.0):
            skipped_fractional += 1
            print(f"  [{pid[:8]}] SKIP fractional resolved_to={actual_f}")
            continue
        resolved_at = (
            r.get("question", {}).get("resolution_date", "2024-12-31")
            + "T23:59:59Z"
        )
        actual_b = actual_f >= 0.5
        ok, msg = resolve_one(args.port, pid, actual_b, resolved_at)
        if ok:
            added += 1
            print(f"  [{pid[:8]}] resolve actual={actual_b} ({resolved_at})")
        else:
            failed.append((pid, msg))
            print(f"  [{pid[:8]}] FAIL {msg}", file=sys.stderr)
        # Slight pacing so we don't slam the substrate
        time.sleep(0.1)

    print()
    print("--- summary ---")
    print(f"  added:               {added}")
    print(f"  skipped (already):   {skipped_already}")
    print(f"  skipped (fractional): {skipped_fractional}")
    print(f"  failed:              {len(failed)}")

    # Post-run: report calibration store state.
    after_seen = already_resolved(history_path)
    print(f"\ncalibration store now has {len(after_seen)} program_ids")
    if bias_path.exists():
        try:
            bias = json.loads(bias_path.read_text())
            print(f"bias.json: a={bias.get('a'):.4f}, b={bias.get('b'):.4f}")
        except Exception:
            print(f"bias.json exists but couldn't parse")
    else:
        print(f"bias.json not yet present (need ≥10 observations to fit Platt)")

    if failed:
        print("\nFailures:", file=sys.stderr)
        for pid, msg in failed:
            print(f"  {pid}: {msg}", file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
