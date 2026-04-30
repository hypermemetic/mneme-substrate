# Live marketplace pipeline (Manifold)

A pair of scripts that turn the substrate into a continuous live
forecaster against [Manifold](https://manifold.markets) markets.

## Operation

```bash
# In mneme-substrate/, with the containerized substrate running:
python3 scripts/marketwatch_live.py     # forecast pass
python3 scripts/marketwatch_resolve.py  # resolution sweeper
```

`marketwatch_live.py` selects open binary markets matching a filter,
fires `forecast.update` for each (using the substrate's iterative
loop), and logs paired `(manifold_p, our_p)` rows to
`programs/_marketwatch/pairings.jsonl`.

`marketwatch_resolve.py` checks Manifold's API for any market we
forecasted that has now resolved, records the outcome in
`programs/_marketwatch/resolutions.jsonl`, and feeds each prior
forecast to the substrate's calibration store via `forecast.resolve`.

## How it decides whether to refresh a forecast

For each candidate market:

- First time we see it → forecast.
- Otherwise, refresh only if **either**:
  - The Manifold price has moved by `--price-delta-threshold` (default 0.05)
  - The last forecast is older than `--time-threshold-hours` (default 24)
- Else: skip; the prior forecast is still valid.

This makes repeated invocations idempotent within thresholds, so it's
safe to cron.

## Filter knobs

```bash
python3 scripts/marketwatch_live.py \
  --max-markets 20 \
  --min-volume 200 \
  --close-min 14 --close-max 60 \
  --price-delta-threshold 0.05 \
  --time-threshold-hours 24 \
  --port 4456 \
  --trials 2 --iterative-max-steps 5 \
  --concurrency 6
```

## Up/down lifecycle (resilience, MNEME-33)

The pipeline is designed to survive periodic substrate downtime — the
laptop closing, container restart, host migration. Three guarantees:

**1. Orphan-forecast recovery.** If `marketwatch_live.py` is killed
between `forecast.update` returning and the pairing row being
written, the next invocation scans `programs/` for any
`MANIFOLD-LIVE-*` programs that have completed but aren't in
`pairings.jsonl`. It reconstructs the pairing row from the artifact
and appends it with `ts_recovered: true`. Idempotent.

**2. Two-phase resolution.** `marketwatch_resolve.py` is split:

- **Phase A** — network-only. Hits Manifold for each unresolved
  market, writes any new resolution to `resolutions.jsonl` with
  `fed_calibration: pending`. Does **not** require the substrate.
- **Phase B** — substrate-only. Reads `resolutions.jsonl`, finds rows
  with `fed_calibration in {pending, failed}`, and calls
  `forecast.resolve` for each forecast we made on that market.
  Records each attempt in `_calibration_feeds.jsonl`. Idempotent —
  re-running retries failures without re-feeding successes.

```bash
python3 scripts/marketwatch_resolve.py                      # both phases
python3 scripts/marketwatch_resolve.py --phase a-only       # substrate down
python3 scripts/marketwatch_resolve.py --phase b-only       # retry feeds
```

If you run with the substrate down, use `--phase a-only`. When the
substrate comes back up, run `--phase b-only` (or default `both`) to
flush pending feeds.

**3. Manifold market deletion.** A 404 / 410 / empty body from
Manifold's market endpoint is logged as "gone from manifold" and the
sweep continues. The pairing remains in `pairings.jsonl`; the next
sweep will retry. (If a market is permanently gone, manually purge
its pairing row.)

**4. Substrate liveness check.** Both scripts probe `synapse -P <port>
substrate _info` at startup and exit with a clear error if the
substrate isn't reachable. (`marketwatch_resolve.py --phase a-only`
skips this check.)

## Integration test

`scripts/test_marketwatch_resilience.sh` exercises the resilience
behaviors end-to-end against a running substrate:

```bash
./scripts/test_marketwatch_resilience.sh
```

It seeds a synthetic orphan, verifies recovery, verifies idempotence,
verifies 404 handling on a nonexistent market, and verifies that
`--phase a-only` doesn't require the substrate while `--phase b-only`
refuses to run without it. Exits non-zero on any failed assertion.

## Suggested cron schedule

```cron
# every 4 hours, refresh forecasts on watched markets
0 */4 * * * cd /Users/shmendez/dev/controlflow/hypermemetic/mneme-substrate && python3 scripts/marketwatch_live.py --max-markets 20 >> /tmp/marketwatch.log 2>&1

# every day at 6am, sweep for newly-resolved markets
0 6 * * * cd /Users/shmendez/dev/controlflow/hypermemetic/mneme-substrate && python3 scripts/marketwatch_resolve.py >> /tmp/marketwatch_resolve.log 2>&1
```

The substrate must be running in the container (port 4456) for both
scripts to work.

## Data files

`programs/_marketwatch/` (gitignored under `programs/`) contains:

| file | what |
|---|---|
| `pairings.jsonl` | append-only log of every forecast we made + the manifold crowd price at that moment |
| `resolutions.jsonl` | append-only log of markets that have resolved (Phase A writes one row per resolved market) |
| `_calibration_feeds.jsonl` | append-only log of every `forecast.resolve` attempt (Phase B). Most recent `(market_id, program_id)` row wins on retry. |
| `selected.json` | last invocation's market selection (for diagnostic / debug) |

A row in pairings.jsonl looks like:

```json
{
  "ts": "2026-04-30T01:30:00+00:00",
  "ts_unix": 1777513800,
  "market_id": "I9On8Elh",
  "market_url": "https://manifold.markets/...",
  "question": "Will X happen by Y?",
  "close_time": "2026-05-15T...",
  "manifold_p": 0.942,
  "our_p": 0.368,
  "our_raw_p": 0.383,
  "program_id": "abc-123",
  "n_trials": 2,
  "summary": "FOR: ... AGAINST: ... OPEN: ..."
}
```

## Calibration loop

Over time, as markets resolve:

```
forecast.update      → pairings.jsonl row
                       (our_p, manifold_p, ts)
        ↓
   (wait days/weeks)
        ↓
  market resolves on Manifold
        ↓
marketwatch_resolve.py → resolutions.jsonl row
                       → forecast.resolve in substrate
        ↓
   substrate's calibration store grows with REAL out-of-sample
   observations (no contamination — markets resolved AFTER our
   forecast at a freeze that was the "now" at forecast time).
        ↓
   bias.json refits with each resolution; future forecasts
   benefit from the tighter Platt fit.
```

This is the only data path that produces unambiguous calibration
evidence. Everything else (ForecastBench, etc.) has some flavor of
post-hoc selection or training-data contamination.

## What to look for in the data

After a few weeks of operation:

1. **Mean(our_p - manifold_p)** — does mneme systematically over- or
   under-shoot the crowd? On the first 10-market pass we saw -0.058
   (mneme runs ~6 points lower than market). Worth tracking.

2. **Brier delta** — for each resolved market, was mneme's Brier
   lower than the market's freeze-time price's Brier? Positive paired
   delta means mneme adds value.

3. **Per-question-source patterns** — are there categories where
   mneme reliably beats or loses to the market? (Manifold doesn't
   have rich question categorization, but the question text itself
   is searchable.)

4. **Big-disagreement outcomes** — markets where mneme and Manifold
   disagreed sharply (|Δ| > 0.3). These are the highest-information
   resolutions; they tell us which side was right and on what kinds
   of questions.

## Caveats

- Manifold uses **play money**. Crowd is high-quality but not as
  efficient as Polymarket. Treat manifold_p as "smart-amateur consensus"
  not "efficient-market price."
- Some markets resolve `MKT` (proportional resolution) or `N/A`
  (cancelled). The resolve sweeper records these but does not feed
  them to forecast.resolve (which requires a clean bool).
- We forecast with `current_price` as evidence in the prompt. The
  model can be biased toward agreeing with the market. For independent
  signal, consider a flag that strips the price from the prompt and
  has the model reason from scratch.
