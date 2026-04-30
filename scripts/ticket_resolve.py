#!/usr/bin/env python3
"""
Ticket-as-forecast resolver — close the loop on past design predictions.

Reads `<plans-dir>/_predictions.jsonl`, finds predictions whose deadline
has passed and that aren't yet in `_resolutions.jsonl`. For each:
  1. Print the ticket id, hypothesis, deadline, predicted probability,
     and the resolution method.
  2. Prompt the user: YES / NO / SKIP / NA / Q (quit).
  3. On YES/NO: call forecast.resolve via synapse; the calibration
     store grows with one design-judgment row.
  4. Append to `_resolutions.jsonl`.

Usage:
    python3 scripts/ticket_resolve.py
    python3 scripts/ticket_resolve.py --plans-dir <path> --port 4456
"""
import argparse
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_PLANS_DIR = "../mneme/plans"
PREDICTIONS = "_predictions.jsonl"
RESOLUTIONS = "_resolutions.jsonl"


def load_jsonl(path: Path):
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


def already_resolved_ticket_ids(path: Path):
    return {r["ticket_id"] for r in load_jsonl(path) if r.get("ticket_id")}


def parse_deadline(s):
    if not s:
        return None
    try:
        if s.endswith("Z"):
            return datetime.fromisoformat(s[:-1] + "+00:00")
        return datetime.fromisoformat(s)
    except Exception:
        return None


def resolve_in_substrate(program_id, actual_bool, port):
    params = {"program_id": program_id, "actual": actual_bool}
    cmd = ["synapse", "-j", "-P", str(port), "-p", json.dumps(params),
           "substrate", "forecast", "resolve"]
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
    if out.returncode != 0:
        return False, out.stderr[:200]
    for line in out.stdout.splitlines():
        try:
            evt = json.loads(line.strip())
            if evt.get("type") == "data" and evt["content"].get("type") == "resolved":
                return True, "ok"
            if evt.get("type") == "data" and evt["content"].get("type") == "error":
                return False, evt["content"].get("message", "")[:200]
        except Exception:
            continue
    return False, "no resolved event"


def prompt_outcome(predicted_p):
    print()
    print(f"  predicted: {predicted_p:.3f}")
    while True:
        ans = input("  outcome [Y/N/skip/na/q]: ").strip().lower()
        if ans in ("y", "yes"):
            return "YES"
        if ans in ("n", "no"):
            return "NO"
        if ans in ("s", "skip"):
            return "SKIP"
        if ans == "na":
            return "NA"
        if ans in ("q", "quit"):
            return "QUIT"
        print("    please answer Y, N, skip, na, or q")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--plans-dir", default=DEFAULT_PLANS_DIR)
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--include-future", action="store_true",
                    help="also resolve predictions whose deadline hasn't passed")
    args = ap.parse_args()

    plans = Path(args.plans_dir).resolve()
    pred_path = plans / PREDICTIONS
    res_path = plans / RESOLUTIONS

    predictions = load_jsonl(pred_path)
    if not predictions:
        print(f"no predictions in {pred_path}")
        return

    already = already_resolved_ticket_ids(res_path)
    now = datetime.now(timezone.utc)

    candidates = []
    for p in predictions:
        if "error" in p:
            continue
        if p.get("ticket_id") in already:
            continue
        deadline = parse_deadline(p.get("deadline"))
        if not args.include_future and deadline and deadline > now:
            continue
        candidates.append(p)

    print(f"{len(predictions)} total predictions; {len(already)} already resolved; "
          f"{len(candidates)} ready to resolve")
    if not candidates:
        return

    for p in candidates:
        print()
        print("=" * 70)
        print(f"  ticket:        {p['ticket_id']}")
        print(f"  hypothesis:    {p['hypothesis']}")
        print(f"  deadline:      {p.get('deadline', '?')}")
        print(f"  resolved by:   {p.get('resolution_method', '?')}")
        outcome = prompt_outcome(p.get("predicted_p", 0.5))
        if outcome == "QUIT":
            break
        if outcome == "SKIP":
            continue

        record = {
            "ts": now.isoformat(),
            "ticket_id": p["ticket_id"],
            "outcome": outcome,
            "program_id": p.get("program_id"),
            "predicted_p": p.get("predicted_p"),
        }
        if outcome in ("YES", "NO"):
            ok, msg = resolve_in_substrate(p["program_id"], outcome == "YES", args.port)
            record["fed_calibration"] = ok
            record["calibration_msg"] = msg if not ok else "ok"
            if ok:
                print(f"    → forecast.resolve OK; calibration store grew")
            else:
                print(f"    → forecast.resolve FAILED: {msg[:120]}")
        else:
            record["fed_calibration"] = False
            record["calibration_msg"] = f"skipped (outcome={outcome})"
        with res_path.open("a") as f:
            f.write(json.dumps(record) + "\n")

    print()
    print(f"resolutions appended to {res_path}")


if __name__ == "__main__":
    main()
