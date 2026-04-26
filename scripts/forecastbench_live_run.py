#!/usr/bin/env python3
"""
Live mneme-vs-crowd backtest on a vendored ForecastBench release.

For each market question in the joined set:
  1. Fire substrate.forecast.update via synapse (returns program_id immediately)
  2. Poll programs/<id>/artifact.json until completed
  3. Read the predicted probability
  4. Compare to the resolution_set's `resolved_to`

Usage:
  ./programs/_benchmarks/run_live_bench.py \\
      --question-set programs/_benchmarks/forecastbench/2024-07-21-llm.json \\
      --resolution-set programs/_benchmarks/forecastbench/2024-07-21_resolution_set.json \\
      --n 20 --concurrency 4 --port 4456 --trials 2 \\
      --output programs/_benchmarks/runs/$(date +%Y%m%d-%H%M%S)/

Run from inside mneme-substrate/. Substrate must be running on --port.
"""
import argparse
import json
import math
import os
import random
import statistics
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

MARKET_SOURCES = {"manifold", "metaculus", "polymarket", "infer"}


def is_single_id(q):
    return isinstance(q.get("id"), str)


def join_market(question_set_path: Path, resolution_set_path: Path):
    qs = json.loads(question_set_path.read_text())
    rs = json.loads(resolution_set_path.read_text())
    res_by_id = {}
    for r in rs["resolutions"]:
        if r["source"] in MARKET_SOURCES and isinstance(r["id"], str):
            res_by_id[r["id"]] = r
    joined = []
    for q in qs["questions"]:
        if q["source"] not in MARKET_SOURCES:
            continue
        if not is_single_id(q):
            continue
        r = res_by_id.get(q["id"])
        if r is None:
            continue
        joined.append({
            "id": q["id"],
            "source": q["source"],
            "question": q["question"],
            "resolution_criteria": q["resolution_criteria"],
            "freeze_datetime_value": q.get("freeze_datetime_value"),
            "resolution_date": r["resolution_date"],
            "resolved_to": r["resolved_to"],
        })
    joined.sort(key=lambda j: j["resolution_date"])
    return joined


def fire_forecast(question, port: int, trials: int, run_dir: Path) -> dict:
    """Fire one forecast.update against the substrate; poll its artifact."""
    new_evidence = (
        f"Question: {question['question']}\n\n"
        f"Resolution criteria: {question['resolution_criteria']}\n\n"
        f"Source: {question['source']}\n"
        f"As-of market price (freeze_datetime_value): {question['freeze_datetime_value']}\n\n"
        f"Forecast the probability the question resolves YES."
    )
    params = {
        "program_id": f"FBLIVE-{question['id']}",
        "new_evidence": new_evidence,
        "trials": trials,
        "allowed_tools": ["WebSearch"],
    }
    cmd = [
        "synapse", "-j", "-P", str(port), "-p", json.dumps(params),
        "substrate", "forecast", "update",
    ]
    t0 = time.time()
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    if out.returncode != 0:
        return {"id": question["id"], "error": f"synapse returned {out.returncode}: {out.stderr[:300]}"}
    # synapse -j emits NDJSON: each line is `{"type":"data","content":{...}}`
    # or `{"type":"done"}`. The forecast.update Started event lives inside
    # the data envelope's `content.type == "started"`.
    program_id = None
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
        if evt.get("type") == "data":
            content = evt.get("content") or {}
            if isinstance(content, dict) and content.get("type") == "started":
                program_id = content.get("program_id")
                if program_id:
                    break
    if program_id is None:
        return {"id": question["id"], "error": f"no program_id in synapse output: {out.stdout[:500]}"}

    artifact_path = Path("programs") / program_id / "artifact.json"
    error_path = Path("programs") / program_id / "error.json"
    deadline = time.time() + 600  # 10 min/question hard cap
    while time.time() < deadline:
        if artifact_path.exists():
            try:
                artifact = json.loads(artifact_path.read_text())
            except Exception as e:
                return {"id": question["id"], "program_id": program_id, "error": f"artifact parse: {e}"}
            p = artifact.get("probability")
            if p is None:
                return {"id": question["id"], "program_id": program_id, "error": "artifact missing probability"}
            return {
                "id": question["id"],
                "program_id": program_id,
                "predicted": float(p),
                "actual": float(question["resolved_to"]),
                "wall_seconds": time.time() - t0,
                "artifact": artifact,
                "question": question,
            }
        if error_path.exists():
            err = error_path.read_text()
            return {"id": question["id"], "program_id": program_id, "error": f"program error: {err[:300]}"}
        time.sleep(3)
    return {"id": question["id"], "program_id": program_id, "error": "polling timeout (10 min)"}


def brier(p, a):
    return (p - a) ** 2


def brier_index(predictions):
    if not predictions:
        return None
    mb = sum(brier(p, a) for p, a in predictions) / len(predictions)
    return 100.0 * (1.0 - mb / 0.25)


def bootstrap_mean_brier_ci(predictions, n_resamples=1000, conf=0.95, seed=0xC0FFEE):
    if len(predictions) < 2:
        return None
    rng = random.Random(seed)
    n = len(predictions)
    means = []
    for _ in range(n_resamples):
        s = sum(brier(*predictions[rng.randrange(n)]) for _ in range(n))
        means.append(s / n)
    means.sort()
    a = (1 - conf) / 2
    lo = means[int(a * n_resamples)]
    hi = means[min(n_resamples - 1, int((1 - a) * n_resamples))]
    return (lo, hi)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--question-set", required=True)
    ap.add_argument("--resolution-set", required=True)
    ap.add_argument("--n", type=int, default=20)
    ap.add_argument("--concurrency", type=int, default=4)
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--trials", type=int, default=2)
    ap.add_argument("--output", default=None, help="dir to write results.jsonl + summary.json")
    args = ap.parse_args()

    if args.output:
        out_dir = Path(args.output)
        out_dir.mkdir(parents=True, exist_ok=True)
    else:
        out_dir = None

    joined = join_market(Path(args.question_set), Path(args.resolution_set))
    print(f"loaded {len(joined)} joined market questions; running first {args.n} at concurrency {args.concurrency}", flush=True)

    sample = joined[: args.n]
    results = []
    t0 = time.time()
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futs = {pool.submit(fire_forecast, q, args.port, args.trials, out_dir): q for q in sample}
        for fut in as_completed(futs):
            r = fut.result()
            results.append(r)
            if "error" in r:
                print(f"  [{r['id']}] FAIL: {r['error']}", flush=True)
            else:
                print(
                    f"  [{r['id']}] p={r['predicted']:.3f} actual={r['actual']:.3f} "
                    f"brier={brier(r['predicted'], r['actual']):.4f} "
                    f"({r['wall_seconds']:.0f}s)",
                    flush=True,
                )
    elapsed = time.time() - t0

    successes = [r for r in results if "error" not in r]
    failures = [r for r in results if "error" in r]
    print(f"\n--- DONE in {elapsed:.0f}s ({elapsed/60:.1f} min) ---")
    print(f"successes: {len(successes)}, failures: {len(failures)}")

    if successes:
        preds = [(r["predicted"], r["actual"]) for r in successes]
        crowd_preds = [
            (float(r["question"]["freeze_datetime_value"]), r["actual"])
            for r in successes
            if r["question"]["freeze_datetime_value"]
            and not r["question"]["freeze_datetime_value"].startswith("N/A")
        ]
        mb = sum(brier(p, a) for p, a in preds) / len(preds)
        bi = brier_index(preds)
        ci = bootstrap_mean_brier_ci(preds)
        print(f"\nMNEME on n={len(preds)}:")
        print(f"  mean Brier: {mb:.4f}")
        print(f"  Brier Index: {bi:.2f}")
        if ci:
            print(f"  95% CI on mean Brier: [{ci[0]:.4f}, {ci[1]:.4f}]")

        if crowd_preds:
            cmb = sum(brier(p, a) for p, a in crowd_preds) / len(crowd_preds)
            cbi = brier_index(crowd_preds)
            cci = bootstrap_mean_brier_ci(crowd_preds)
            print(f"\nCROWD baseline on the SAME n={len(crowd_preds)} (paired):")
            print(f"  mean Brier: {cmb:.4f}")
            print(f"  Brier Index: {cbi:.2f}")
            if cci:
                print(f"  95% CI on mean Brier: [{cci[0]:.4f}, {cci[1]:.4f}]")
            print(f"\nDelta (mneme - crowd) on mean Brier: {mb - cmb:+.4f}")
            print(f"Delta Brier Index (mneme - crowd):       {(bi or 0) - (cbi or 0):+.2f}")

    if out_dir:
        (out_dir / "results.jsonl").write_text(
            "\n".join(json.dumps(r, default=str) for r in results)
        )
        summary = {
            "n_questions": args.n,
            "concurrency": args.concurrency,
            "trials": args.trials,
            "successes": len(successes),
            "failures": len(failures),
            "wall_seconds": elapsed,
            "mneme_mean_brier": (sum(brier(p, a) for p, a in preds) / len(preds)) if successes else None,
            "mneme_brier_index": brier_index(preds) if successes else None,
        }
        (out_dir / "summary.json").write_text(json.dumps(summary, indent=2))
        print(f"\nresults written to {out_dir}/")


if __name__ == "__main__":
    main()
