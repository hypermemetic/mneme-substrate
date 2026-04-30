#!/usr/bin/env python3
"""
Held-out (post-cutoff) ForecastBench live mneme-vs-crowd backtest.

Variant of forecastbench_live_run.py that joins ONE question_set against the
UNION of multiple resolution_sets. Lets us pick a question_set whose
forecast_due_date is at/before Sonnet's training cutoff, then score against
all resolutions that have landed after that date so we capture every
question that has resolved post-cutoff.

For each market question (manifold/metaculus/polymarket/infer) joined to a
resolution row whose `resolution_date` falls in the requested window:
  1. Fire substrate.forecast.update via synapse (returns program_id immediately)
  2. Poll programs/<id>/artifact.json until completed
  3. Read the predicted probability
  4. Compare to the resolution_set's `resolved_to`

Usage:
  python3 scripts/forecastbench_holdout_run.py \\
      --question-set programs/_benchmarks/forecastbench/2026-03-15-llm.json \\
      --resolution-sets \\
          programs/_benchmarks/forecastbench/2026-03-15_resolution_set.json \\
          programs/_benchmarks/forecastbench/2026-03-29_resolution_set.json \\
          programs/_benchmarks/forecastbench/2026-04-12_resolution_set.json \\
      --resolve-from 2026-03-25 --resolve-to 2026-04-29 \\
      --n 63 --concurrency 6 --port 4456 --trials 2 --iterative-max-steps 5 \\
      --output programs/_benchmarks/runs/<ts>-bench006-holdout-postcutoff/

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


def parseable_float(s):
    """True iff s parses to a finite float. Excludes 'N/A' etc."""
    if s is None:
        return False
    try:
        v = float(s)
    except (TypeError, ValueError):
        return False
    return math.isfinite(v)


def load_resolutions_union(paths):
    """
    Build {id: resolution_row} from the union of multiple resolution_set files.
    Only keeps rows from MARKET_SOURCES with a string id and resolved=True.
    On duplicate ids, keeps the row with the earliest resolution_date (the
    first time the question resolved is the canonical event).
    """
    res_by_id: dict = {}
    for path in paths:
        rs = json.loads(Path(path).read_text())
        for r in rs["resolutions"]:
            if r["source"] not in MARKET_SOURCES:
                continue
            if not isinstance(r["id"], str):
                continue
            if not r.get("resolved"):
                continue
            existing = res_by_id.get(r["id"])
            if existing is None or r.get("resolution_date", "") < existing.get("resolution_date", ""):
                res_by_id[r["id"]] = r
    return res_by_id


def join_market(question_set_path: Path, resolution_set_paths,
                resolve_from: str, resolve_to: str):
    qs = json.loads(question_set_path.read_text())
    res_by_id = load_resolutions_union(resolution_set_paths)
    joined = []
    skipped_no_freeze = 0
    skipped_outside_window = 0
    skipped_no_resolution = 0
    for q in qs["questions"]:
        if q["source"] not in MARKET_SOURCES:
            continue
        if not is_single_id(q):
            continue
        r = res_by_id.get(q["id"])
        if r is None:
            skipped_no_resolution += 1
            continue
        rd = r.get("resolution_date", "")
        if not (resolve_from <= rd <= resolve_to):
            skipped_outside_window += 1
            continue
        if not parseable_float(q.get("freeze_datetime_value")):
            skipped_no_freeze += 1
            continue
        joined.append({
            "id": q["id"],
            "source": q["source"],
            "question": q["question"],
            "resolution_criteria": q["resolution_criteria"],
            "freeze_datetime_value": q["freeze_datetime_value"],
            "resolution_date": rd,
            "resolved_to": r["resolved_to"],
        })
    joined.sort(key=lambda j: j["resolution_date"])
    print(
        f"join: kept {len(joined)} | skipped {skipped_no_resolution} (no resolution), "
        f"{skipped_outside_window} (outside [{resolve_from}, {resolve_to}]), "
        f"{skipped_no_freeze} (unparseable freeze_datetime_value)",
        flush=True,
    )
    return joined


def fire_forecast(question, port: int, trials: int, run_dir: Path,
                  iterative_max_steps: int = 0) -> dict:
    """Fire one forecast.update against the substrate; poll its artifact."""
    new_evidence = (
        f"Question: {question['question']}\n\n"
        f"Resolution criteria: {question['resolution_criteria']}\n\n"
        f"Source: {question['source']}\n"
        f"As-of market price (freeze_datetime_value): {question['freeze_datetime_value']}\n\n"
        f"Forecast the probability the question resolves YES."
    )
    params = {
        "program_id": f"FBHOLD-{question['id']}",
        "new_evidence": new_evidence,
        "trials": trials,
        "allowed_tools": ["WebSearch"],
    }
    if iterative_max_steps and iterative_max_steps > 0:
        params["iterative_max_steps"] = iterative_max_steps
    cmd = [
        "synapse", "-j", "-P", str(port), "-p", json.dumps(params),
        "substrate", "forecast", "update",
    ]
    t0 = time.time()
    out = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    if out.returncode != 0:
        return {"id": question["id"], "error": f"synapse returned {out.returncode}: {out.stderr[:300]}"}
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


def bootstrap_mean_brier_ci(predictions, n_resamples=10000, conf=0.95, seed=0xC0FFEE):
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


def bootstrap_mean_delta_ci(deltas, n_resamples=10000, conf=0.95, seed=0xC0FFEE):
    if len(deltas) < 2:
        return None
    rng = random.Random(seed)
    n = len(deltas)
    means = []
    for _ in range(n_resamples):
        s = sum(deltas[rng.randrange(n)] for _ in range(n))
        means.append(s / n)
    means.sort()
    a = (1 - conf) / 2
    lo = means[int(a * n_resamples)]
    hi = means[min(n_resamples - 1, int((1 - a) * n_resamples))]
    return (lo, hi)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--question-set", required=True)
    ap.add_argument("--resolution-sets", nargs="+", required=True,
                    help="One or more resolution_set.json files; their resolved rows are unioned")
    ap.add_argument("--resolve-from", required=True,
                    help="Lower bound (inclusive) on resolution_date, e.g. 2026-03-25")
    ap.add_argument("--resolve-to", required=True,
                    help="Upper bound (inclusive) on resolution_date, e.g. 2026-04-29")
    ap.add_argument("--n", type=int, default=0,
                    help="If >0, run first n joined questions; default 0 = all")
    ap.add_argument("--concurrency", type=int, default=4)
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--trials", type=int, default=2)
    ap.add_argument("--iterative-max-steps", type=int, default=0)
    ap.add_argument("--output", default=None)
    ap.add_argument("--wallclock-cap-seconds", type=int, default=5400,
                    help="Hard cap on bench wall-clock; partial results written if exceeded")
    args = ap.parse_args()

    if args.output:
        out_dir = Path(args.output)
        out_dir.mkdir(parents=True, exist_ok=True)
    else:
        out_dir = None

    joined = join_market(
        Path(args.question_set), [Path(p) for p in args.resolution_sets],
        args.resolve_from, args.resolve_to,
    )
    sample = joined if args.n <= 0 else joined[: args.n]
    print(f"running {len(sample)} questions at concurrency {args.concurrency}, "
          f"trials={args.trials}, iterative_max_steps={args.iterative_max_steps}", flush=True)

    if out_dir:
        # Persist the join manifest for reproducibility.
        (out_dir / "joined_questions.json").write_text(
            json.dumps(sample, indent=2)
        )
        (out_dir / "config.json").write_text(json.dumps({
            "question_set": str(args.question_set),
            "resolution_sets": list(args.resolution_sets),
            "resolve_from": args.resolve_from,
            "resolve_to": args.resolve_to,
            "n": args.n,
            "concurrency": args.concurrency,
            "trials": args.trials,
            "iterative_max_steps": args.iterative_max_steps,
            "port": args.port,
        }, indent=2))

    if len(sample) < 10:
        print(f"\nABORT: fewer than 10 joined questions ({len(sample)}). "
              "Refusing to run a tiny bench. Try a different window or "
              "another question_set.", flush=True)
        sys.exit(2)

    results = []
    t0 = time.time()
    deadline = t0 + args.wallclock_cap_seconds
    capped = False
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool:
        futs = {
            pool.submit(fire_forecast, q, args.port, args.trials, out_dir,
                        args.iterative_max_steps): q
            for q in sample
        }
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
            if time.time() > deadline:
                capped = True
                print(f"\n!! wall-clock cap of {args.wallclock_cap_seconds}s "
                      "exceeded — cancelling remaining futures", flush=True)
                for f in futs:
                    f.cancel()
                break
    elapsed = time.time() - t0

    successes = [r for r in results if "error" not in r]
    failures = [r for r in results if "error" in r]
    print(f"\n--- DONE in {elapsed:.0f}s ({elapsed/60:.1f} min) ---")
    if capped:
        print(f"  WARNING: hit wall-clock cap; partial results")
    print(f"successes: {len(successes)}, failures: {len(failures)}")

    summary_extra = {}
    if successes:
        # Mneme stats.
        mneme_preds = [(r["predicted"], r["actual"]) for r in successes]
        mb = sum(brier(p, a) for p, a in mneme_preds) / len(mneme_preds)
        bi = brier_index(mneme_preds)
        ci = bootstrap_mean_brier_ci(mneme_preds)
        print(f"\nMNEME on n={len(mneme_preds)}:")
        print(f"  mean Brier: {mb:.4f}")
        print(f"  Brier Index: {bi:.2f}")
        if ci:
            print(f"  95% CI on mean Brier: [{ci[0]:.4f}, {ci[1]:.4f}]")

        # Crowd paired (we already filtered to parseable freeze in the join).
        crowd_preds = [
            (float(r["question"]["freeze_datetime_value"]), r["actual"])
            for r in successes
        ]
        cmb = sum(brier(p, a) for p, a in crowd_preds) / len(crowd_preds)
        cbi = brier_index(crowd_preds)
        cci = bootstrap_mean_brier_ci(crowd_preds)
        print(f"\nCROWD baseline (paired) on n={len(crowd_preds)}:")
        print(f"  mean Brier: {cmb:.4f}")
        print(f"  Brier Index: {cbi:.2f}")
        if cci:
            print(f"  95% CI on mean Brier: [{cci[0]:.4f}, {cci[1]:.4f}]")

        # Paired delta.
        deltas = [
            brier(r["predicted"], r["actual"])
            - brier(float(r["question"]["freeze_datetime_value"]), r["actual"])
            for r in successes
        ]
        mean_delta = sum(deltas) / len(deltas)
        delta_ci = bootstrap_mean_delta_ci(deltas)
        bi_delta = (bi or 0) - (cbi or 0)
        wins = sum(1 for d in deltas if d < -0.01)
        losses = sum(1 for d in deltas if d > 0.01)
        ties = len(deltas) - wins - losses
        print(f"\nPAIRED delta (mneme - crowd) on n={len(deltas)}:")
        print(f"  mean Brier delta: {mean_delta:+.4f}")
        print(f"  BI delta: {bi_delta:+.2f}")
        if delta_ci:
            print(f"  95% CI on paired mean delta: [{delta_ci[0]:+.4f}, {delta_ci[1]:+.4f}]")
            if delta_ci[1] < 0:
                print(f"  -> mneme beats crowd DECISIVELY (CI excludes 0 on the negative side)")
            elif delta_ci[0] > 0:
                print(f"  -> crowd beats mneme DECISIVELY (CI excludes 0 on the positive side)")
            else:
                print(f"  -> CI includes 0; cannot distinguish at p<0.05")
        print(f"  per-question outcomes (|delta|>0.01): mneme wins {wins}, crowd wins {losses}, ties {ties}")

        # Per-source breakdown.
        by_src = {}
        for r in successes:
            s = r["question"]["source"]
            by_src.setdefault(s, []).append((r["predicted"], r["actual"]))
        print("\nPer-source mneme breakdown:")
        for s, ps in by_src.items():
            smb = sum(brier(p, a) for p, a in ps) / len(ps)
            print(f"  {s:<12} n={len(ps):>3} mean_brier={smb:.4f} BI={brier_index(ps):.2f}")

        summary_extra = {
            "mneme_mean_brier": mb,
            "mneme_brier_index": bi,
            "mneme_ci": list(ci) if ci else None,
            "crowd_mean_brier": cmb,
            "crowd_brier_index": cbi,
            "crowd_ci": list(cci) if cci else None,
            "paired_mean_delta": mean_delta,
            "paired_bi_delta": bi_delta,
            "paired_delta_ci": list(delta_ci) if delta_ci else None,
            "wins": wins, "losses": losses, "ties": ties,
            "per_source": {
                s: {"n": len(ps),
                    "mean_brier": sum(brier(p,a) for p,a in ps)/len(ps),
                    "brier_index": brier_index(ps)}
                for s, ps in by_src.items()
            },
        }

    if out_dir:
        (out_dir / "results.jsonl").write_text(
            "\n".join(json.dumps(r, default=str) for r in results)
        )
        summary = {
            "n_joined": len(joined),
            "n_attempted": len(sample),
            "successes": len(successes),
            "failures": len(failures),
            "wall_seconds": elapsed,
            "wallclock_capped": capped,
            **summary_extra,
        }
        (out_dir / "summary.json").write_text(json.dumps(summary, indent=2, default=str))
        print(f"\nresults written to {out_dir}/")


if __name__ == "__main__":
    main()
