#!/usr/bin/env python3
"""
Paired analysis: bench-004a (single-shot at λ=0.0 + Platt) vs bench-004b
(iterative T_max=5 at λ=0.0 + Platt). Same 20 questions in both.

Joins by question_id, computes per-question Brier delta, paired-bootstrap
95% CI on the mean delta and on the BI delta.

Usage:
    python3 scripts/bench_004_paired_analysis.py \\
        --a programs/_benchmarks/runs/<004a>/results.jsonl \\
        --b programs/_benchmarks/runs/<004b>/results.jsonl
"""
import argparse
import json
import random
import sys
from pathlib import Path


def load(path: Path):
    out = {}
    with path.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            if "error" in r:
                continue
            out[r["id"]] = r
    return out


def brier(p, a):
    return (p - a) ** 2


def bi(mean_b):
    return 100.0 * (1.0 - mean_b / 0.25)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--a", required=True, help="bench-004a results.jsonl")
    ap.add_argument("--b", required=True, help="bench-004b results.jsonl")
    ap.add_argument("--n-resamples", type=int, default=10000)
    args = ap.parse_args()

    a = load(Path(args.a))
    b = load(Path(args.b))
    common = sorted(set(a.keys()) & set(b.keys()))
    print(f"a: {len(a)} records, b: {len(b)} records, paired: {len(common)}")

    paired = []
    for qid in common:
        ra = a[qid]
        rb = b[qid]
        actual_a = float(ra["actual"])
        actual_b = float(rb["actual"])
        assert actual_a == actual_b, f"mismatched actual for {qid}"
        paired.append((qid, ra["predicted"], rb["predicted"], actual_a))

    # Per-question table.
    print()
    print(f"{'qid':<20}  {'ss-p':>6}  {'it-p':>6}  {'actual':>6}  {'ss-brier':>9}  {'it-brier':>9}  {'delta':>8}")
    for qid, p_a, p_b, actual in paired:
        ba = brier(p_a, actual)
        bb = brier(p_b, actual)
        print(f"{qid[:20]:<20}  {p_a:>6.3f}  {p_b:>6.3f}  {actual:>6.1f}  {ba:>9.4f}  {bb:>9.4f}  {bb-ba:>+8.4f}")

    # Paired summary.
    deltas = [brier(pb, a) - brier(pa, a) for _, pa, pb, a in paired]
    mean_delta = sum(deltas) / len(deltas)
    mean_b_a = sum(brier(pa, ac) for _, pa, _, ac in paired) / len(paired)
    mean_b_b = sum(brier(pb, ac) for _, _, pb, ac in paired) / len(paired)

    print()
    print("--- summary ---")
    print(f"  single-shot (a): mean Brier = {mean_b_a:.4f}, BI = {bi(mean_b_a):.2f}")
    print(f"  iterative   (b): mean Brier = {mean_b_b:.4f}, BI = {bi(mean_b_b):.2f}")
    print(f"  paired delta (b - a): mean = {mean_delta:+.4f}")
    print(f"  paired BI delta (b - a): {bi(mean_b_b) - bi(mean_b_a):+.2f}")

    # Paired bootstrap CI on the mean delta.
    rng = random.Random(0xC0FFEE)
    n = len(deltas)
    means = []
    for _ in range(args.n_resamples):
        s = sum(deltas[rng.randrange(n)] for _ in range(n))
        means.append(s / n)
    means.sort()
    lo = means[int(0.025 * args.n_resamples)]
    hi = means[min(args.n_resamples - 1, int(0.975 * args.n_resamples))]
    print(f"  95% CI on paired mean delta: [{lo:+.4f}, {hi:+.4f}]")
    if hi < 0:
        print(f"  → iterative beats single-shot DECISIVELY (CI excludes 0 on the negative side)")
    elif lo > 0:
        print(f"  → single-shot beats iterative DECISIVELY (CI excludes 0 on the positive side)")
    else:
        print(f"  → CI includes 0 — cannot distinguish at p<0.05")


if __name__ == "__main__":
    main()
