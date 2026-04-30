#!/usr/bin/env python3
"""
ForecastBench submission runner — fires `forecast.update` against a
ForecastBench question_set, writes results, and emits a single
submission JSON file in the format the FRI auto-leaderboard ingests.

Resumable: skips questions that already have a successful artifact
under `programs/<id>/`, keyed off the question_program_id pattern
`FBSUBMIT-<question_id>`. Re-running just retries failures.

Usage:
    # Probe — first 50 market questions
    python3 scripts/forecastbench_submit.py \\
      --question-set programs/_benchmarks/forecastbench/2026-04-26-llm.json \\
      --max-questions 50 --sources manifold,metaculus,polymarket,infer \\
      --output programs/_benchmarks/runs/<ts>-probe-2026-04-26/

    # Full submission (ForecastBench round)
    python3 scripts/forecastbench_submit.py \\
      --question-set programs/_benchmarks/forecastbench/<round>-llm.json \\
      --output programs/_benchmarks/runs/<ts>-submit-<round>/ \\
      --organization hypermemetic --model mneme-v0.x

The output dir contains:
  - config.json              — what was asked of the substrate
  - results.jsonl            — per-question (id, predicted, ts, program_id, error?)
  - submission.json          — ForecastBench format, ready to upload
  - resolved/<id>.json       — per-question reasoning trace summaries

Designed to be safe against rate limits: trial failures don't crash
the run, they're recorded as errors and the script continues.
Re-running picks up where it left off.
"""
import argparse
import json
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from pathlib import Path

PROGRAMS_ROOT = Path("programs")
URL_RE = re.compile(r"https?://[^\s\)\],]+")


def derive_blocked_urls(question) -> list:
    """BLFX-9 layer 4: resolution_criteria contains the page that IS
    the answer. Block it. Substring match — domain prefixes work too."""
    text = question.get("resolution_criteria", "") or ""
    return list(set(URL_RE.findall(text)))


def existing_artifact_for(question_id: str) -> Path | None:
    """Return the artifact path of a prior successful program for this
    question_id, if one exists."""
    target = f"FBSUBMIT-{question_id}"
    if not PROGRAMS_ROOT.exists():
        return None
    # Scan once per call is fine for ~500 questions; if it ever hurts
    # we cache a {question_program_id → program_id} map.
    for child in PROGRAMS_ROOT.iterdir():
        if not child.is_dir() or child.name.startswith("_"):
            continue
        manifest_path = child / "manifest.json"
        if not manifest_path.exists():
            continue
        try:
            m = json.loads(manifest_path.read_text())
        except Exception:
            continue
        if m.get("inputs", {}).get("question_program_id") != target:
            continue
        if m.get("status") != "completed":
            continue
        artifact = child / "artifact.json"
        if artifact.exists():
            return artifact
    return None


def fire_forecast(question, port: int, trials: int, iterative_max_steps: int,
                  cutoff_date: str | None) -> dict:
    """Fire one forecast.update; return our result row."""
    new_evidence = (
        f"Question: {question['question']}\n\n"
        f"Resolution criteria: {question.get('resolution_criteria', '')}\n\n"
        f"Source: {question.get('source', '?')}\n"
    )
    if "freeze_datetime_value" in question:
        new_evidence += f"As-of crowd estimate (freeze_datetime_value): {question['freeze_datetime_value']}\n\n"
    else:
        new_evidence += "\n"
    new_evidence += "Forecast the probability the question resolves YES."

    params = {
        "program_id": f"FBSUBMIT-{question['id']}",
        "new_evidence": new_evidence,
        "trials": trials,
        "allowed_tools": ["WebSearch"],
    }
    if iterative_max_steps and iterative_max_steps > 0:
        params["iterative_max_steps"] = iterative_max_steps
    if cutoff_date:
        params["cutoff_date"] = cutoff_date
        blocked = derive_blocked_urls(question)
        if blocked:
            params["blocked_urls"] = blocked

    cmd = ["synapse", "-j", "-P", str(port), "-p", json.dumps(params),
           "substrate", "forecast", "update"]
    t0 = time.time()
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        return {"id": question["id"], "error": "synapse fire timed out (60s)"}
    if out.returncode != 0:
        return {"id": question["id"], "error": f"synapse exit {out.returncode}: {out.stderr[:300]}"}

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
        return {"id": question["id"], "error": f"no program_id in synapse output"}

    artifact_path = PROGRAMS_ROOT / program_id / "artifact.json"
    error_path = PROGRAMS_ROOT / program_id / "error.json"
    deadline = time.time() + 720  # 12 min/question
    while time.time() < deadline:
        if artifact_path.exists():
            try:
                artifact = json.loads(artifact_path.read_text())
            except Exception as e:
                return {"id": question["id"], "program_id": program_id,
                        "error": f"artifact parse: {e}"}
            p = artifact.get("probability")
            if p is None:
                return {"id": question["id"], "program_id": program_id,
                        "error": "artifact missing probability"}
            return {
                "id": question["id"],
                "program_id": program_id,
                "predicted": float(p),
                "raw_probability": artifact.get("raw_probability"),
                "summary": artifact.get("summary", "")[:500],
                "wall_seconds": time.time() - t0,
                "question_source": question.get("source"),
                "resolution_dates": question.get("resolution_dates"),
            }
        if error_path.exists():
            return {"id": question["id"], "program_id": program_id,
                    "error": f"program error: {error_path.read_text()[:300]}"}
        time.sleep(3)
    return {"id": question["id"], "program_id": program_id,
            "error": "polling timeout (12 min)"}


def fire_or_reuse(question, port: int, trials: int, iterative_max_steps: int,
                  cutoff_date: str | None) -> dict:
    """Resumability: if a successful prior artifact exists, reuse it."""
    art = existing_artifact_for(question["id"])
    if art is not None:
        try:
            artifact = json.loads(art.read_text())
            return {
                "id": question["id"],
                "program_id": art.parent.name,
                "predicted": float(artifact["probability"]),
                "raw_probability": artifact.get("raw_probability"),
                "summary": artifact.get("summary", "")[:500],
                "wall_seconds": 0.0,
                "reused": True,
                "question_source": question.get("source"),
                "resolution_dates": question.get("resolution_dates"),
            }
        except Exception:
            pass  # fall through and re-fire
    return fire_forecast(question, port, trials, iterative_max_steps, cutoff_date)


def to_submission_row(result: dict) -> dict | None:
    """Map our result row to the FRI submission schema."""
    if "error" in result:
        return None
    # ForecastBench expects one row per (question_id, resolution_date).
    # Dataset questions have multiple resolution_dates; market questions
    # usually have a single (or null) resolution_date. We emit one row
    # per resolution_date with the same probability — the agent
    # produced one number per question.
    rds = result.get("resolution_dates") or [None]
    rows = []
    for rd in rds:
        rows.append({
            "id": result["id"],
            "source": result["question_source"],
            "forecast": result["predicted"],
            "resolution_date": rd,
            "reasoning": result.get("summary", "")[:500],
        })
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--question-set", required=True)
    ap.add_argument("--output", required=True)
    ap.add_argument("--max-questions", type=int, default=0,
                    help="If >0, run only the first N (after source filter)")
    ap.add_argument("--sources", default="manifold,metaculus,polymarket,infer",
                    help="Comma-separated source allow-list (default: market sources only)")
    ap.add_argument("--port", type=int, default=4456)
    ap.add_argument("--trials", type=int, default=2)
    ap.add_argument("--iterative-max-steps", type=int, default=5)
    ap.add_argument("--concurrency", type=int, default=4)
    ap.add_argument("--cutoff-date", default=None,
                    help="ISO 8601 cutoff for BLFX-9 layers 1+2+4 enforcement. "
                         "If you're submitting predictions for FUTURE resolutions "
                         "this should generally be None (the freeze IS now).")
    ap.add_argument("--organization", default="hypermemetic")
    ap.add_argument("--model", default="mneme-v0.x")
    ap.add_argument("--model-organization", default="anthropic")
    ap.add_argument("--wallclock-cap-seconds", type=int, default=14400,
                    help="Hard cap; partial submission written if exceeded")
    args = ap.parse_args()

    out_dir = Path(args.output)
    out_dir.mkdir(parents=True, exist_ok=True)
    qs = json.loads(Path(args.question_set).read_text())

    sources = {s.strip() for s in args.sources.split(",") if s.strip()}
    questions = [q for q in qs.get("questions", []) if q.get("source") in sources]
    print(f"question set: {qs.get('forecast_due_date')}  total: {len(qs.get('questions', []))}", flush=True)
    print(f"after source filter ({sources}): {len(questions)}", flush=True)
    if args.max_questions > 0:
        questions = questions[:args.max_questions]
        print(f"capped to first {len(questions)}", flush=True)

    config = {
        "question_set": str(args.question_set),
        "forecast_due_date": qs.get("forecast_due_date"),
        "n_questions": len(questions),
        "sources": sorted(sources),
        "concurrency": args.concurrency,
        "trials": args.trials,
        "iterative_max_steps": args.iterative_max_steps,
        "cutoff_date": args.cutoff_date,
        "organization": args.organization,
        "model": args.model,
        "model_organization": args.model_organization,
        "started_at": datetime.now(timezone.utc).isoformat(),
    }
    (out_dir / "config.json").write_text(json.dumps(config, indent=2))

    results_path = out_dir / "results.jsonl"
    # Resumability at the script level too — read prior results, skip
    # those question_ids on this invocation.
    already_done_ids = set()
    if results_path.exists():
        for line in results_path.open():
            line = line.strip()
            if not line: continue
            try:
                r = json.loads(line)
                if "error" not in r:
                    already_done_ids.add(r["id"])
            except Exception:
                continue
        print(f"resume: {len(already_done_ids)} questions already done in this run dir", flush=True)
    todo = [q for q in questions if q["id"] not in already_done_ids]
    print(f"running {len(todo)} fresh questions at concurrency {args.concurrency}", flush=True)

    t0 = time.time()
    deadline = t0 + args.wallclock_cap_seconds
    capped = False
    consec_429 = 0
    with ThreadPoolExecutor(max_workers=args.concurrency) as pool, \
         results_path.open("a") as results_f:
        futs = {pool.submit(fire_or_reuse, q, args.port, args.trials,
                            args.iterative_max_steps, args.cutoff_date): q
                for q in todo}
        for fut in as_completed(futs):
            q = futs[fut]
            r = fut.result()
            results_f.write(json.dumps(r) + "\n")
            results_f.flush()
            if "error" in r:
                msg = r["error"][:80]
                # Crude 429 detection — if we see "429" or "rate" or "limit"
                # several times in a row, back off.
                if any(s in r["error"].lower() for s in ("429", "rate limit", "rate_limit")):
                    consec_429 += 1
                else:
                    consec_429 = 0
                print(f"  [{r['id'][:14]}] FAIL ({q.get('source')}): {msg}", flush=True)
                if consec_429 >= 5:
                    print(f"\n!! 5 consecutive rate-limit failures — sleeping 15min before retrying", flush=True)
                    time.sleep(15 * 60)
                    consec_429 = 0
            else:
                tag = "REUSED" if r.get("reused") else f"{r['wall_seconds']:.0f}s"
                print(f"  [{r['id'][:14]}] p={r['predicted']:.3f} ({q.get('source')}, {tag})", flush=True)
                consec_429 = 0
            if time.time() > deadline:
                capped = True
                print(f"\n!! wall-clock cap of {args.wallclock_cap_seconds}s exceeded", flush=True)
                for f in futs:
                    f.cancel()
                break
    elapsed = time.time() - t0

    # Build submission.json from results.jsonl (single source of truth)
    forecasts = []
    successes = 0
    failures = []
    with results_path.open() as f:
        for line in f:
            line = line.strip()
            if not line: continue
            r = json.loads(line)
            if "error" in r:
                failures.append(r)
                continue
            successes += 1
            rows = to_submission_row(r) or []
            forecasts.extend(rows)
    submission = {
        "organization": args.organization,
        "model": args.model,
        "model_organization": args.model_organization,
        "question_set": qs.get("forecast_due_date"),
        "forecasts": forecasts,
    }
    submission_path = out_dir / "submission.json"
    submission_path.write_text(json.dumps(submission, indent=2))

    print(f"\n--- DONE in {elapsed:.0f}s ({elapsed/60:.1f} min) ---")
    if capped:
        print(f"  WARNING: wall-clock cap hit; partial submission")
    print(f"  successes: {successes}")
    print(f"  failures:  {len(failures)}")
    print(f"  forecasts in submission.json: {len(forecasts)} rows")
    if successes > 0:
        coverage = successes / len(questions) * 100
        print(f"  coverage:  {coverage:.1f}% of {len(questions)} eligible questions")
        print(f"             (FRI requires ≥95% for leaderboard scoring)")
    print(f"\n  results:    {results_path}")
    print(f"  submission: {submission_path}")


if __name__ == "__main__":
    main()
