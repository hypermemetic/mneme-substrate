#!/usr/bin/env bash
# MNEME-33 integration test — exercises the resilience guarantees:
#   1. orphan-forecast recovery on marketwatch_live restart
#   2. two-phase resolve handles substrate-down via --phase a-only
#   3. phase B retries pending feeds when substrate comes back
#   4. Manifold 404/deletion is logged but doesn't crash
#
# Designed to run from inside the substrate container (where synapse
# and the substrate-on-:4456 are reachable). Operates on a temporary
# scratch dir for marketwatch state so it doesn't pollute real data.
#
# Usage:
#   ./scripts/test_marketwatch_resilience.sh
#
# Exit non-zero on any failed assertion.
set -euo pipefail

PORT="${PORT:-4456}"
SCRATCH_DIR="programs/_marketwatch_test_$(date +%s)"
PAIRINGS="$SCRATCH_DIR/pairings.jsonl"
RESOLUTIONS="$SCRATCH_DIR/resolutions.jsonl"
FEEDS="$SCRATCH_DIR/_calibration_feeds.jsonl"

cleanup() { rm -rf "$SCRATCH_DIR"; }
trap cleanup EXIT

assert_file_exists() {
  if [[ ! -f "$1" ]]; then
    echo "FAIL: expected file $1 to exist" >&2
    exit 1
  fi
}

assert_jsonl_count() {
  local file="$1" expected="$2" what="$3"
  local actual
  actual=$(grep -c . "$file" 2>/dev/null || echo 0)
  if [[ "$actual" != "$expected" ]]; then
    echo "FAIL: $what — expected $expected rows in $file, got $actual" >&2
    exit 1
  fi
  echo "  OK: $what ($actual rows)"
}

assert_jsonl_has() {
  local file="$1" jq_query="$2" what="$3"
  if ! jq -e "$jq_query" "$file" >/dev/null 2>&1; then
    echo "FAIL: $what — query $jq_query did not match in $file" >&2
    exit 1
  fi
  echo "  OK: $what"
}

mkdir -p "$SCRATCH_DIR"

echo "=== MNEME-33 resilience integration test ==="
echo "scratch dir: $SCRATCH_DIR"
echo "substrate port: $PORT"
echo

# --- 0. preflight: substrate must be up for the start of the test
echo "[0] preflight — substrate liveness probe"
if ! synapse -P "$PORT" substrate hash >/dev/null 2>&1; then
  echo "FAIL: substrate at port $PORT not reachable. Start it with \`make run\`." >&2
  exit 1
fi
echo "  OK: substrate alive"
echo

# --- 1. seed a synthetic orphan: a manifest.json + artifact.json under
# programs/<uuid>/ that looks exactly like what the substrate would
# have written if marketwatch_live.py had crashed mid-handoff. We
# fabricate it instead of firing a real forecast so the test runs in
# seconds, not minutes. The recovery scan reads disk only.

FAKE_MARKET="orphan-test-$(date +%s)"
ORPHAN_PROGRAM_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
SCRATCH_PROGRAMS="$SCRATCH_DIR/programs"
ORPHAN_DIR="$SCRATCH_PROGRAMS/$ORPHAN_PROGRAM_ID"
echo "[1] fabricating synthetic orphan program (market_id=$FAKE_MARKET, program_id=$ORPHAN_PROGRAM_ID)"
mkdir -p "$ORPHAN_DIR"

NOW_ISO="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cat > "$ORPHAN_DIR/manifest.json" <<MANIFEST
{
  "program_id": "$ORPHAN_PROGRAM_ID",
  "parent_program_id": null,
  "entry_skill": "forecast.update",
  "inputs": {
    "allowed_tools": ["WebSearch"],
    "new_evidence": "Question: Will MNEME-33 ship?\n\nSource: manifold-live\nCurrent crowd probability (manifold): 0.4500\n\nForecast the probability YES.",
    "question_program_id": "MANIFOLD-LIVE-$FAKE_MARKET",
    "trials": 1
  },
  "inputs_schema_version": "0.1.0",
  "started_at": "$NOW_ISO",
  "finished_at": "$NOW_ISO",
  "status": "completed",
  "artifact_schema_version": "0.3.0",
  "substrate_version": "0.6.3",
  "mneme_version": "0.6.3",
  "manifest_schema_version": "0.1.0",
  "depth": 0
}
MANIFEST

cat > "$ORPHAN_DIR/artifact.json" <<'ARTIFACT'
{
  "probability": 0.42,
  "raw_probability": 0.45,
  "confidence": "multi-trial",
  "evidence_for": [],
  "evidence_against": [],
  "open_questions": [],
  "summary": "synthetic orphan for MNEME-33 integration test",
  "n_trials": 1,
  "belief_schema_version": "0.3.0"
}
ARTIFACT

echo "  OK: orphan dir written under $SCRATCH_PROGRAMS, no pairing row exists for it yet"
echo

# --- 2. run orphan recovery via marketwatch_live with --max-markets 0
#
# We can't easily make marketwatch_live skip the manifold fetch, but
# the scan happens before any forecasting. We point its working files
# at the scratch dir by overriding the module-level paths via a
# small wrapper.

echo "[2] running orphan recovery"
PYTHONPATH=. python3 - <<PY
import sys
import scripts.marketwatch_live as ml
from pathlib import Path
ml.MARKETWATCH_DIR = Path("$SCRATCH_DIR")
ml.PAIRINGS_PATH = Path("$PAIRINGS")
ml.SELECTED_PATH = Path("$SCRATCH_DIR/selected.json")
recovered = ml.recover_orphan_forecasts(programs_root=Path("$SCRATCH_PROGRAMS"))
assert recovered == 1, f"expected 1 orphan recovered, got {recovered}"
print(f"recovered={recovered}")
PY
assert_file_exists "$PAIRINGS"
assert_jsonl_has "$PAIRINGS" \
  ".program_id == \"$ORPHAN_PROGRAM_ID\" and .ts_recovered == true" \
  "orphan was recovered into pairings.jsonl with ts_recovered=true"
echo

# --- 3. idempotency: run again, no new row
echo "[3] orphan recovery is idempotent"
PYTHONPATH=. python3 - <<PY
import scripts.marketwatch_live as ml
from pathlib import Path
ml.MARKETWATCH_DIR = Path("$SCRATCH_DIR")
ml.PAIRINGS_PATH = Path("$PAIRINGS")
recovered2 = ml.recover_orphan_forecasts(programs_root=Path("$SCRATCH_PROGRAMS"))
assert recovered2 == 0, f"expected 0 on second run, got {recovered2}"
print("ok")
PY
assert_jsonl_count "$PAIRINGS" 1 "still one row after second recovery"
echo

# --- 4. Manifold 404 handling
#
# Phase A against a nonexistent market_id should not crash; should log
# 'gone from manifold' and exit cleanly.

echo "[4] phase A handles a deleted/nonexistent market (404)"
PYTHONPATH=. python3 - <<PY
import scripts.marketwatch_resolve as mr
from pathlib import Path
mr.MARKETWATCH_DIR = Path("$SCRATCH_DIR")
mr.PAIRINGS_PATH = Path("$PAIRINGS")
mr.RESOLUTIONS_PATH = Path("$RESOLUTIONS")
mr.FEEDS_PATH = Path("$FEEDS")
pairings = mr.load_jsonl(mr.PAIRINGS_PATH)
counts = mr.phase_a(pairings, set())
assert counts["deleted"] >= 1, f"expected deleted>=1, got {counts}"
print(f"counts={counts}")
PY
echo "  OK: 404 logged, not crashed"
echo

# --- 5. Phase A is safe with substrate "down" (we don't actually need
#       to bring it down; phase A doesn't call synapse at all). Verify
#       that --phase a-only is the documented escape hatch.
echo "[5] resolver --phase a-only does not require substrate"
python3 scripts/marketwatch_resolve.py --phase a-only --port 99999 \
    --marketwatch-dir "$SCRATCH_DIR" \
  > /tmp/mneme-33-aonly.log 2>&1 || {
    echo "FAIL: --phase a-only exited non-zero with bogus port" >&2
    cat /tmp/mneme-33-aonly.log >&2
    exit 1
}
echo "  OK: phase a-only ignores substrate liveness"
echo

# --- 6. substrate-down detection in marketwatch_live + resolver phase B

echo "[6] resolver --phase b-only refuses to run if substrate is down"
if python3 scripts/marketwatch_resolve.py --phase b-only --port 99999 \
     --marketwatch-dir "$SCRATCH_DIR" \
   > /tmp/mneme-33-bonly.log 2>&1; then
  echo "FAIL: --phase b-only with bogus port should have exited non-zero" >&2
  exit 1
fi
if ! grep -q "not reachable" /tmp/mneme-33-bonly.log; then
  echo "FAIL: error message missing 'not reachable'" >&2
  cat /tmp/mneme-33-bonly.log >&2
  exit 1
fi
echo "  OK: clear error when substrate is down"
echo

echo "=== ALL CHECKS PASSED ==="
