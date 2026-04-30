#!/usr/bin/env python3
"""
Re-run a specific subset of failed questions from a previous bench run
against the (newly deployed) substrate. Used to validate that a recovery
fix (e.g. lenient EvidenceItem parser, MNEME-29 cleanup) actually
resolves the failure pattern.

Usage:
    python3 scripts/rerun_failed.py --joined <path> --ids id1 id2 ... \\
        [--port 4456] [--trials 2] [--iterative-max-steps 5] [--concurrency 4]
"""
import argparse
import json
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path


def load_joined(path: Path):
    return {q["id"]: q for q in json.loads(path.read_text())}


def fire(question, port, trials, iterative):
    new_evidence = (
        f"Question: {question['question']}\n\n"
        f"Resolution criteria: {question['resolution_criteria']}\n\n"
        f"Source: {question['source']}\n"
        f"As-of market price (freeze_datetime_value): {question['freeze_datetime_value']}\n\n"
        f"Forecast the probability the question resolves YES."
    )
    params = {
        "program_id": f"RERUN-{question['id']}",
        "new_evidence": new_evidence,
        "trials": trials,
        "allowed_tools": ["WebSearch"],
    }
    if iterative > 0:
        params["iterative_max_steps"] = iterative
    cmd = ["synapse", "-j", "-P", str(port), "-p", json.dumps(params),
           "substrate", "forecast", "update"]
    t0 = time.time()
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    if out.returncode != 0:
        return {"id": question["id"], "error": f"synapse: {out.stderr[:200]}"}
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
        return {"id": question["id"], "error": "no program_id"}
    artifact = Path("programs") / program_id / "artifact.json"
    error_path = Path("programs") / program_id / "error.json"
    deadline = time.time() + 720
    while time.time() < deadline:
        if artifact.exists():
            try:
                a = json.loads(artifact.read_text())
                return {
                    "id": question["id"],
                    "program_id": program_id,
                    "predicted": a.get("probability"),
                    "raw_predicted": a.get("raw_probability"),
                    "actual": float(question["resolved_to"]),
                    "wall_seconds": time.time() - t0,
                }
            except Exception as e:
                return {"id": question["id"], "error": f"artifact parse: {e}"}
        if error_path.exists():
            return {"id": question["id"], "program_id": program_id,
                    "error": f"program error: {error_path.read_text()[:200]}"}
        time.sleep(3)
    return {"id": question["id"], "program_id": program_id, "error": "timeout"}


def brier(p, a):
    return (p - a) ** 2


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--joined", required=True, help="path to joined_questions.json from prior run")
    ap.add_argument("--ids", nargs="+", required=True)
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--trials", type=int, default=2)
    ap.add_argument("--iterative-max-steps", type=int, default=5)
    ap.add_argument("--concurrency", type=int, default=4)
    args = ap.parse_args()

    joined = load_joined(Path(args.joined))
    questions = [joined[i] for i in args.ids if i in joined]
    print(f"running {len(questions)} questions at concurrency {args.concurrency}")

    results = []
    t0 = time.time()
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futs = {pool.submit(fire, q, args.port, args.trials, args.iterative_max_steps): q for q in questions}
        for fut in as_completed(futs):
            r = fut.result()
            results.append(r)
            if "error" in r:
                print(f"  [{r['id'][:30]}] FAIL: {r['error'][:120]}")
            else:
                b = brier(r["predicted"], r["actual"])
                raw = r.get("raw_predicted")
                raw_str = f" raw={raw:.3f}" if raw is not None else ""
                print(f"  [{r['id'][:30]}] p={r['predicted']:.3f}{raw_str} actual={r['actual']:.3f} brier={b:.4f} ({r['wall_seconds']:.0f}s)")
    elapsed = time.time() - t0
    succ = [r for r in results if "error" not in r]
    fail = [r for r in results if "error" in r]
    print(f"\n--- DONE in {elapsed:.0f}s ---")
    print(f"  recovered: {len(succ)} / {len(questions)}")
    print(f"  still failing: {len(fail)}")


if __name__ == "__main__":
    main()
