#!/usr/bin/env python3
"""
MNEME-35: queryable reasoning-chain inspector.

For a given mneme program id, walks every layer of the audit trail
into a single readable tree and prints it. No second join required
from the operator.

Layers walked:

  1. programs/<id>/manifest.json     — inputs (prompt, trials, BLFX-9 cutoff, ...)
  2. programs/<id>/artifact.json     — final probability + structured belief state
  3. programs/<id>/trace.jsonl       — op-level trace, gives child_session_ids
  4. programs/<id>/trial_<k>_history.jsonl
                                     — per-step (action, observation, belief);
                                       persisted by run_iterative_trial
                                       (only iterative trials)
  5. .plexus-state/substrate/activations/claudecode/claudecode.db
                                     — full role+content per chat turn,
                                       joined by trial_session_name

Usage:
    python3 scripts/inspect_program_tree.py <program_id>
    python3 scripts/inspect_program_tree.py <program_id> --json
    python3 scripts/inspect_program_tree.py <program_id> --turns

Flags:
    --json    emit a single JSON object (programmatic consumers)
    --turns   include the full chat turns (default: omit; they can be very large)
    --max-content N
              when rendering, truncate any single content/observation/snippet
              to N chars (default: 240). --max-content 0 disables truncation.

Exit codes:
    0   tree rendered successfully
    1   program_id not found / invalid
    2   I/O error reading state
"""
import argparse
import json
import sqlite3
import sys
import textwrap
from pathlib import Path

PROGRAMS_DIR = Path("programs")
CLAUDECODE_DB = Path(".plexus-state/substrate/activations/claudecode/claudecode.db")


def load_program_files(program_id: str):
    pdir = PROGRAMS_DIR / program_id
    if not pdir.is_dir():
        return None
    out = {"program_id": program_id, "program_dir": str(pdir)}

    manifest_path = pdir / "manifest.json"
    if manifest_path.exists():
        out["manifest"] = json.loads(manifest_path.read_text())
    else:
        return None  # no manifest = no program

    art = pdir / "artifact.json"
    if art.exists():
        out["artifact"] = json.loads(art.read_text())

    err = pdir / "error.json"
    if err.exists():
        out["error"] = json.loads(err.read_text())

    trace_path = pdir / "trace.jsonl"
    out["trace"] = []
    if trace_path.exists():
        for line in trace_path.open():
            line = line.strip()
            if not line:
                continue
            try:
                out["trace"].append(json.loads(line))
            except Exception:
                continue

    return out


def collect_trial_session_names(prog: dict):
    """Walk trace entries, return all child_session_ids whose name
    looks like a trial session for this program."""
    pid = prog["program_id"]
    seen = []
    seen_set = set()
    for entry in prog.get("trace", []):
        for sid in entry.get("child_session_ids", []) or []:
            if sid.startswith(f"{pid}-trial-") and sid not in seen_set:
                seen.append(sid)
                seen_set.add(sid)
    return seen


def load_trial_history(program_id: str, trial_index: int):
    path = PROGRAMS_DIR / program_id / f"trial_{trial_index}_history.jsonl"
    if not path.exists():
        return []
    rows = []
    for line in path.open():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except Exception:
            continue
    return rows


def load_chat_turns(session_name: str):
    if not CLAUDECODE_DB.exists():
        return None
    con = sqlite3.connect(f"file:{CLAUDECODE_DB}?mode=ro", uri=True)
    try:
        row = con.execute(
            "SELECT id FROM claudecode_sessions WHERE name = ?", (session_name,)
        ).fetchone()
        if not row:
            return []
        session_id = row[0]
        cur = con.execute(
            "SELECT role, content, model_id, input_tokens, output_tokens, cost_usd, created_at "
            "FROM claudecode_messages WHERE session_id = ? ORDER BY created_at ASC",
            (session_id,),
        )
        out = []
        for r, c, mid, in_t, out_t, cost, ts in cur:
            out.append({
                "role": r,
                "content": c,
                "model_id": mid,
                "input_tokens": in_t,
                "output_tokens": out_t,
                "cost_usd": cost,
                "created_at": ts,
            })
        return out
    finally:
        con.close()


def build_tree(program_id: str, include_turns: bool):
    prog = load_program_files(program_id)
    if prog is None:
        return None

    trial_sessions = collect_trial_session_names(prog)
    trials = []
    for sname in trial_sessions:
        # session name is "<program_id>-trial-<k>"; pull k
        try:
            k = int(sname.rsplit("-", 1)[-1])
        except ValueError:
            k = -1
        trial = {
            "trial_index": k,
            "session_name": sname,
            "step_history": load_trial_history(program_id, k) if k >= 0 else [],
        }
        if include_turns:
            trial["chat_turns"] = load_chat_turns(sname) or []
        trials.append(trial)
    trials.sort(key=lambda t: t["trial_index"])
    prog["trials"] = trials
    return prog


def truncate(s: str, n: int) -> str:
    if not isinstance(s, str):
        return str(s)
    if n <= 0 or len(s) <= n:
        return s
    return s[:n] + f"…[+{len(s)-n}c]"


def render_evidence_list(tag: str, items, indent: str, max_chars: int):
    if not items:
        print(f"{indent}{tag}: (none)")
        return
    print(f"{indent}{tag}:")
    for ev in items:
        if isinstance(ev, dict):
            claim = ev.get("claim", "")
            src = ev.get("source", "")
            wt = ev.get("weight")
            wt_str = f"  w={wt:.2f}" if isinstance(wt, (int, float)) else ""
            print(f"{indent}  · {truncate(claim, max_chars)}{wt_str}")
            if src:
                print(f"{indent}    src: {truncate(src, max_chars)}")
        else:
            print(f"{indent}  · {truncate(str(ev), max_chars)}")


def render_tree(tree: dict, max_chars: int, show_turns: bool):
    pid = tree["program_id"]
    print(f"PROGRAM {pid}")
    m = tree.get("manifest", {})
    print(f"  entry_skill:    {m.get('entry_skill')}")
    print(f"  status:         {m.get('status')}")
    print(f"  started_at:     {m.get('started_at')}")
    print(f"  finished_at:    {m.get('finished_at')}")
    inputs = m.get("inputs") or {}
    print(f"  inputs:")
    for k, v in inputs.items():
        if k == "new_evidence":
            print(f"    new_evidence: {truncate(v, max_chars)}")
        elif k == "blocked_urls" and v:
            print(f"    blocked_urls: [{len(v)} URLs]")
            for u in v[:3]:
                print(f"      · {truncate(u, max_chars)}")
            if len(v) > 3:
                print(f"      … +{len(v)-3} more")
        else:
            print(f"    {k}: {truncate(json.dumps(v), max_chars) if not isinstance(v, str) else truncate(v, max_chars)}")

    art = tree.get("artifact")
    if art:
        print()
        print("ARTIFACT")
        prob = art.get("probability")
        raw = art.get("raw_probability")
        conf = art.get("confidence")
        n_trials = art.get("n_trials")
        if prob is not None:
            raw_str = f"  raw={raw:.4f}" if isinstance(raw, (int, float)) else ""
            print(f"  probability: {prob:.4f}{raw_str}  confidence: {conf}  n_trials: {n_trials}")
        render_evidence_list("evidence_for", art.get("evidence_for") or [], "  ", max_chars)
        render_evidence_list("evidence_against", art.get("evidence_against") or [], "  ", max_chars)
        oqs = art.get("open_questions") or []
        if oqs:
            print(f"  open_questions:")
            for q in oqs:
                print(f"    ? {truncate(q, max_chars)}")
        else:
            print(f"  open_questions: (none — flag for over-confidence)")
        s = art.get("summary")
        if s:
            print(f"  summary: {truncate(s, max_chars)}")
    elif tree.get("error"):
        print()
        print(f"ERROR: {truncate(json.dumps(tree['error']), max_chars)}")

    trace = tree.get("trace", [])
    if trace:
        print()
        print(f"TRACE ({len(trace)} entries)")
        for e in trace:
            print(f"  {e.get('seq'):>3} {e.get('op'):<14}  outcome={e.get('outcome')}  "
                  f"duration_ms={e.get('duration_ms')}  child_sessions={len(e.get('child_session_ids') or [])}")

    trials = tree.get("trials", [])
    print()
    print(f"TRIALS ({len(trials)})")
    for t in trials:
        print(f"  TRIAL {t['trial_index']}  session={t['session_name']}")
        sh = t.get("step_history") or []
        if sh:
            print(f"    step_history ({len(sh)} steps — persisted by run_iterative_trial):")
            for step in sh:
                act = step.get("action") or {}
                obs = step.get("observation") or {}
                bel = step.get("belief") or {}
                act_type = act.get("type", "?")
                act_brief = ""
                if act_type == "web_search":
                    act_brief = f"  q={truncate(act.get('query', ''), 60)}  k={act.get('k')}"
                elif act_type == "lookup_url":
                    act_brief = f"  url={truncate(act.get('url', ''), 80)}"
                elif act_type == "submit":
                    act_brief = f"  p={act.get('probability')}"
                obs_type = obs.get("type", "?")
                obs_brief = ""
                if obs_type == "search_results":
                    obs_brief = f"  ({len(obs.get('results') or [])} hits)"
                elif obs_type == "page_content":
                    obs_brief = f"  ({len(obs.get('content') or '')} chars)"
                elif obs_type == "error":
                    obs_brief = f"  err={truncate(obs.get('message', ''), 60)}"
                bel_p = bel.get("probability")
                bel_str = f"  bel.p={bel_p:.3f}" if isinstance(bel_p, (int, float)) else ""
                print(f"      step {step.get('step_idx')}: action={act_type}{act_brief}")
                print(f"               observation={obs_type}{obs_brief}{bel_str}")
        else:
            print(f"    step_history: (none — single-shot trial OR pre-MNEME-35 program)")
        if show_turns:
            ct = t.get("chat_turns") or []
            if ct:
                print(f"    chat_turns ({len(ct)}):")
                for i, msg in enumerate(ct):
                    role = msg.get("role", "?")
                    content = truncate((msg.get("content") or "").replace("\n", "  ↵  "), max_chars)
                    tok = ""
                    if msg.get("input_tokens") or msg.get("output_tokens"):
                        tok = f"  in={msg.get('input_tokens')}/out={msg.get('output_tokens')}"
                    print(f"      [{i:>2} {role:<10}{tok}] {content}")
            else:
                print(f"    chat_turns: (no rows in claudecode.db for this session)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("program_id", help="program UUID (the directory name in programs/)")
    ap.add_argument("--json", action="store_true",
                    help="emit a single structured JSON object instead of a rendered tree")
    ap.add_argument("--turns", action="store_true",
                    help="include the full per-trial chat turns (large; off by default)")
    ap.add_argument("--max-content", type=int, default=240,
                    help="truncate long strings (claim, snippet, content) to N chars (default 240; 0 disables)")
    args = ap.parse_args()

    tree = build_tree(args.program_id, include_turns=args.turns or args.json)
    if tree is None:
        print(f"ERROR: program {args.program_id} not found under {PROGRAMS_DIR}/", file=sys.stderr)
        sys.exit(1)

    if args.json:
        print(json.dumps(tree, indent=2))
    else:
        render_tree(tree, args.max_content, args.turns)


if __name__ == "__main__":
    main()
