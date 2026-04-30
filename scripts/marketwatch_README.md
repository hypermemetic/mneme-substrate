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
| `resolutions.jsonl` | append-only log of markets that have resolved + which of our forecasts were fed to the calibration store |
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
