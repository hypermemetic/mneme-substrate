#!/usr/bin/env python3
"""
Ticket-as-forecast Phase 1 — turn each ticket with a `forecast:`
frontmatter block into a recorded prediction.

For every ticket file under `<mneme-repo>/plans/<EPIC>/*.md` whose
YAML frontmatter has a `forecast:` block:
  1. Build a prompt = the ticket body + the explicit hypothesis +
     deadline + resolution_method.
  2. Fire `forecast.update` via synapse against the substrate.
  3. Record the prediction in `<mneme-repo>/plans/_predictions.jsonl`.

Idempotent — re-running skips tickets that already have a recorded
prediction (matched by ticket_id).

Usage:
    python3 scripts/ticket_forecast.py
    python3 scripts/ticket_forecast.py --plans-dir /path/to/plans \\
        --port 4456 --trials 3 --iterative-max-steps 5
"""
import argparse
import json
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

DEFAULT_PLANS_DIR = "../mneme/plans"
PREDICTIONS_FILE = "_predictions.jsonl"


def parse_ticket(path: Path):
    """Parse YAML frontmatter + markdown body. Returns (frontmatter, body) or (None, None)."""
    text = path.read_text()
    if not text.startswith("---"):
        return None, None
    parts = text.split("---", 2)
    if len(parts) < 3:
        return None, None
    fm_text = parts[1]
    body = parts[2].strip()
    # Lightweight YAML parse — no dependency on PyYAML, just handle the
    # subset our tickets use.
    fm = {}
    current_key = None
    current_block = None  # for nested 'forecast:' block
    for line in fm_text.splitlines():
        if not line.strip() or line.strip().startswith("#"):
            continue
        if line.startswith("  ") and current_block is not None:
            # nested
            m = re.match(r"^\s+([a-z_]+):\s*(.*)$", line)
            if m:
                k, v = m.group(1), m.group(2).strip()
                if v.startswith('"') and v.endswith('"'):
                    v = v[1:-1]
                current_block[k] = v
            continue
        m = re.match(r"^([a-z_]+):\s*(.*)$", line)
        if not m:
            continue
        key, val = m.group(1), m.group(2).strip()
        if key == "forecast" and not val:
            fm[key] = {}
            current_block = fm[key]
            current_key = key
            continue
        current_block = None
        # Strip quotes if present
        if val.startswith('"') and val.endswith('"'):
            val = val[1:-1]
        # Lists
        if val.startswith("[") and val.endswith("]"):
            inner = val[1:-1].strip()
            fm[key] = [s.strip() for s in inner.split(",")] if inner else []
        elif val == "":
            fm[key] = None
        else:
            fm[key] = val
    return fm, body


def already_predicted(predictions_path: Path):
    if not predictions_path.exists():
        return set()
    out = set()
    with predictions_path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                p = json.loads(line)
                if p.get("ticket_id"):
                    out.add(p["ticket_id"])
            except Exception:
                continue
    return out


def fire_forecast(ticket_id, hypothesis, body, fc_block, port, trials, iterative):
    """Fire one forecast.update for a ticket, return the result dict."""
    prompt = (
        f"You are forecasting the outcome of a software design decision recorded as ticket {ticket_id}.\n\n"
        f"=== HYPOTHESIS ===\n{hypothesis}\n\n"
        f"=== RESOLUTION METHOD ===\n{fc_block.get('resolution_method', '(not specified)')}\n\n"
        f"=== DEADLINE ===\n{fc_block.get('deadline', '(not specified)')}\n\n"
        f"=== TICKET BODY (the actual design proposal) ===\n{body[:6000]}\n\n"
        f"Forecast the probability the hypothesis resolves YES by the deadline. "
        f"Pay attention to: feasibility of the work in the time given, dependencies, "
        f"the resolution method's strictness, and any prior evidence in the ticket body."
    )
    params = {
        "program_id": f"TICKET-FORECAST-{ticket_id}",
        "new_evidence": prompt,
        "trials": trials,
        "allowed_tools": ["WebSearch"],
    }
    if iterative > 0:
        params["iterative_max_steps"] = iterative
    cmd = ["synapse", "-j", "-P", str(port), "-p", json.dumps(params),
           "substrate", "forecast", "update"]
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
        return {"error": "no program_id"}
    artifact = Path("programs") / program_id / "artifact.json"
    error_path = Path("programs") / program_id / "error.json"
    deadline = time.time() + 720
    while time.time() < deadline:
        if artifact.exists():
            try:
                a = json.loads(artifact.read_text())
                return {
                    "program_id": program_id,
                    "predicted_p": a.get("probability"),
                    "raw_predicted_p": a.get("raw_probability"),
                    "summary": a.get("summary", "")[:500],
                    "n_trials": a.get("n_trials"),
                }
            except Exception as e:
                return {"error": f"artifact parse: {e}", "program_id": program_id}
        if error_path.exists():
            return {"error": f"program error: {error_path.read_text()[:200]}",
                    "program_id": program_id}
        time.sleep(3)
    return {"error": "polling timeout", "program_id": program_id}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--plans-dir", default=DEFAULT_PLANS_DIR,
                    help="path to plans/ directory (relative to substrate dir)")
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--trials", type=int, default=2)
    ap.add_argument("--iterative-max-steps", type=int, default=5)
    ap.add_argument("--concurrency", type=int, default=3)
    ap.add_argument("--ticket-id", default=None,
                    help="if set, only forecast this one ticket (re-fires even if already predicted)")
    args = ap.parse_args()

    plans = Path(args.plans_dir).resolve()
    if not plans.exists():
        print(f"ERROR: plans dir {plans} not found", file=sys.stderr)
        sys.exit(1)
    predictions_path = plans / PREDICTIONS_FILE
    seen = already_predicted(predictions_path) if not args.ticket_id else set()

    # Walk all tickets
    candidates = []
    for ticket_path in sorted(plans.glob("**/*.md")):
        # Skip results docs
        if "results" in ticket_path.parts:
            continue
        # Skip _predictions.jsonl etc
        if ticket_path.name.startswith("_"):
            continue
        fm, body = parse_ticket(ticket_path)
        if fm is None:
            continue
        ticket_id = fm.get("id")
        if not ticket_id:
            continue
        if args.ticket_id and ticket_id != args.ticket_id:
            continue
        fc = fm.get("forecast")
        if not fc or not isinstance(fc, dict):
            continue
        hypothesis = fc.get("hypothesis")
        if not hypothesis:
            continue
        if ticket_id in seen:
            print(f"  [{ticket_id}] skip: already predicted")
            continue
        candidates.append((ticket_id, hypothesis, body, fc, ticket_path))

    if not candidates:
        print("nothing to forecast (no unpredicted tickets with forecast: blocks)")
        return

    print(f"firing {len(candidates)} ticket forecasts at concurrency {args.concurrency}", flush=True)
    results = []
    t0 = time.time()
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futs = {
            pool.submit(fire_forecast, tid, hyp, body, fc,
                        args.port, args.trials, args.iterative_max_steps): (tid, hyp, fc, path)
            for (tid, hyp, body, fc, path) in candidates
        }
        for fut in as_completed(futs):
            tid, hyp, fc, path = futs[fut]
            res = fut.result()
            row = {
                "ts": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "ticket_id": tid,
                "ticket_path": str(path),
                "hypothesis": hyp,
                "deadline": fc.get("deadline"),
                "resolution_method": fc.get("resolution_method"),
            }
            row.update(res)
            results.append(row)
            with predictions_path.open("a") as f:
                f.write(json.dumps(row) + "\n")
            if "error" in res:
                print(f"  [{tid}] FAIL: {res['error'][:120]}", flush=True)
            else:
                p = res["predicted_p"]
                raw = res.get("raw_predicted_p")
                raw_str = f" raw={raw:.3f}" if raw is not None else ""
                print(f"  [{tid}] predicted_p={p:.3f}{raw_str}  ({hyp[:80]}...)", flush=True)

    elapsed = time.time() - t0
    succ = [r for r in results if "error" not in r]
    fail = [r for r in results if "error" in r]
    print(f"\n--- DONE in {elapsed:.0f}s ---")
    print(f"  predictions recorded: {len(succ)}")
    if fail:
        print(f"  failed: {len(fail)}")
    print(f"  predictions file: {predictions_path}")


if __name__ == "__main__":
    main()
