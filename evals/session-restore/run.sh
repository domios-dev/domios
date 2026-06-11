#!/usr/bin/env bash
# Session-restore bench — full kill/restart drill against live dev daemon.
# Cross-platform via evals/_lib/lib.sh (mac/linux/win-bash all supported).
# See ./README.md for the test plan and the bug it guards against.
#
# Usage:
#   ./run.sh              # run once, write snapshots + latest-summary.json
#   ./run.sh --check      # run and fail on regression vs baseline.json
#   ./run.sh --check --baseline ./path/to/baseline.json

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASELINE_PATH="$SCRIPT_DIR/baseline.json"
CHECK_MODE=0

usage() {
  cat <<'EOF'
Usage:
  ./run.sh [--check] [--baseline PATH]

Options:
  --check           Compare latest session-restore run against baseline.json.
  --baseline PATH   Baseline file for --check (default: baseline.json)
  -h, --help        Show this help.

Tolerance policy:
  WS-* scenario workspace/pane metadata is exact.
  General workspace tab/pane counts may vary by ±1 because the protected
  General agent can settle with one or two tabs around the forced restart.
EOF
}

while [[ $# -gt 0 ]]; do
  case "${1:-}" in
    --check)
      CHECK_MODE=1
      shift
      ;;
    --baseline)
      shift
      BASELINE_PATH="${1:?--baseline needs path}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown arg: $1" >&2
      usage
      exit 1
      ;;
  esac
done

source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"
dev_env || exit 1
dev_alive || { echo "ERROR: dev not running. Start it: ./build.sh dev" >&2; exit 1; }

compare_against_baseline() {
  local pre="$1" post="$2" baseline="$3" out="$4" check_mode="$5"
  PYTHONIOENCODING=utf-8 python - "$pre" "$post" "$baseline" "$out" "$check_mode" <<'PY'
import json
import os
import sys

pre, post, baseline_path, out_path, check_mode = sys.argv[1:6]

def read(path, name):
    with open(os.path.join(path, name), "r", encoding="utf-8") as f:
        return json.load(f)

def index_ws(workspaces):
    return {w["name"]: w for w in workspaces}

def panes_by_workspace(panes):
    grouped = {}
    for pane in panes:
        grouped.setdefault(pane.get("workspace_name"), []).append(pane)
    return grouped

def slim_pane(pane):
    return {
        "title": pane.get("title"),
        "agent_type": pane.get("agent_type"),
        "cwd": pane.get("cwd"),
        "tab_title": pane.get("tab_title"),
        "title_level": pane.get("title_level"),
    }

pre_ws = read(pre, "ws.json")
post_ws = read(post, "ws.json")
pre_panes = read(pre, "panes.json")
post_panes = read(post, "panes.json")
pre_ws_by_name = index_ws(pre_ws)
post_ws_by_name = index_ws(post_ws)
pre_panes_by_ws = panes_by_workspace(pre_panes)
post_panes_by_ws = panes_by_workspace(post_panes)

scenario_names = sorted(name for name in pre_ws_by_name if name.startswith("WS-"))

summary = {
    "workspace_count": {"pre": len(pre_ws), "post": len(post_ws)},
    "pane_count": {"pre": len(pre_panes), "post": len(post_panes)},
    "scenarios": {},
    "general": {},
}

for name in scenario_names:
    before = pre_ws_by_name[name]
    after = post_ws_by_name.get(name)
    before_panes = sorted(
        (slim_pane(p) for p in pre_panes_by_ws.get(name, [])),
        key=lambda p: (p.get("title") or "", p.get("cwd") or ""),
    )
    after_panes = sorted(
        (slim_pane(p) for p in post_panes_by_ws.get(name, [])),
        key=lambda p: (p.get("title") or "", p.get("cwd") or ""),
    )
    summary["scenarios"][name] = {
        "cwd": before.get("cwd"),
        "pre_tab_count": before.get("tab_count"),
        "post_tab_count": after.get("tab_count") if after else None,
        "pre_pane_count": len(before_panes),
        "post_pane_count": len(after_panes),
        "pre_panes": before_panes,
        "post_panes": after_panes,
    }

general_pre = pre_ws_by_name.get("General")
general_post = post_ws_by_name.get("General")
if general_pre and general_post:
    summary["general"] = {
        "pre_tab_count": general_pre.get("tab_count"),
        "post_tab_count": general_post.get("tab_count"),
        "pre_pane_count": len(pre_panes_by_ws.get("General", [])),
        "post_pane_count": len(post_panes_by_ws.get("General", [])),
    }

with open(out_path, "w", encoding="utf-8") as f:
    json.dump(summary, f, indent=2, sort_keys=True)
    f.write("\n")

print(f"  summary: {out_path}")
print(f"  scenario workspaces: {len(summary['scenarios'])}")
print(
    f"  workspaces: pre={summary['workspace_count']['pre']} "
    f"post={summary['workspace_count']['post']}"
)
print(f"  panes:      pre={summary['pane_count']['pre']} post={summary['pane_count']['post']}")

if check_mode != "1":
    sys.exit(0)

if not os.path.exists(baseline_path):
    print(f"ERROR: baseline not found: {baseline_path}", file=sys.stderr)
    sys.exit(1)

with open(baseline_path, "r", encoding="utf-8") as f:
    baseline = json.load(f)

expected = baseline["expected"]
tolerances = baseline.get("tolerances", {})
general_tab_tol = int(tolerances.get("general_tab_count_abs", 0))
general_pane_tol = int(tolerances.get("general_pane_count_abs", 0))

failures = []

for key in ("pre", "post"):
    got = summary["workspace_count"][key]
    want = expected["workspace_count"][key]
    if got != want:
        failures.append(f"workspace_count.{key}: {got} != {want}")

got_scenarios = set(summary["scenarios"])
want_scenarios = set(expected["scenarios"])
if got_scenarios != want_scenarios:
    failures.append(
        "scenario set mismatch: "
        f"missing={sorted(want_scenarios - got_scenarios)} "
        f"extra={sorted(got_scenarios - want_scenarios)}"
    )

for name, want in expected["scenarios"].items():
    got = summary["scenarios"].get(name)
    if not got:
        continue
    for key in ("cwd", "pre_tab_count", "post_tab_count", "pre_pane_count", "post_pane_count", "pre_panes", "post_panes"):
        if got.get(key) != want.get(key):
            failures.append(f"{name}.{key}: {got.get(key)!r} != {want.get(key)!r}")

for key in ("pre_tab_count", "post_tab_count"):
    got = summary["general"].get(key)
    want = expected["general"].get(key)
    if got is None or abs(got - want) > general_tab_tol:
        failures.append(f"General.{key}: {got} outside {want} ±{general_tab_tol}")

for key in ("pre_pane_count", "post_pane_count"):
    got = summary["general"].get(key)
    want = expected["general"].get(key)
    if got is None or abs(got - want) > general_pane_tol:
        failures.append(f"General.{key}: {got} outside {want} ±{general_pane_tol}")

print(f"baseline: {baseline_path}")
print(
    "tolerance: WS-* exact; "
    f"General tabs ±{general_tab_tol}, General panes ±{general_pane_tol}"
)

if failures:
    print("FAIL: session-restore regression beyond tolerance")
    for failure in failures:
        print(f"  - {failure}")
    sys.exit(1)

print("PASS: session-restore baseline check within tolerance")
PY
}

echo "== reset dev HARD (kill, blank last.json, restart) for guaranteed clean baseline"
reset_dev_state_hard

echo "== create 6 scenarios"
WS_E=$(new_workspace WS-empty C:/temp/restore-WS-empty)
WS_SS=$(new_workspace WS-shell-silent C:/temp/restore-WS-shell-silent)
WS_ST=$(new_workspace WS-shell-with-text C:/temp/restore-WS-shell-with-text)
WS_CS=$(new_workspace WS-claude-silent C:/temp/restore-WS-claude-silent)
WS_CT=$(new_workspace WS-claude-with-text C:/temp/restore-WS-claude-with-text)
WS_CR=$(new_workspace WS-claude-renamed C:/temp/restore-WS-claude-renamed)

P_SS=$(new_shell_pane $WS_SS C:/temp/restore-WS-shell-silent silent-shell)

P_ST=$(new_shell_pane $WS_ST C:/temp/restore-WS-shell-with-text shell-with-text)
# pwsh PTY linked != shell-ready-for-input. await_pty_link returns
# as soon as the PTY is registered, but the shell process inside
# (powershell.exe) takes another 5-10s to render its prompt and
# attach stdin. exec before that point gets bytes-eaten silently.
# 12s is the empirical safe window on Win11/PS5.
sleep 12 && dc pane exec --pane $P_ST 'echo MARKER_SHELL_BEFORE_KILL' > /dev/null 2>&1

P_CS=$(new_claude_pane $WS_CS C:/temp/restore-WS-claude-silent silent-claude)

P_CT=$(new_claude_pane $WS_CT C:/temp/restore-WS-claude-with-text claude-with-text 'reply with single word PINEAPPLEMARKER')

P_CR=$(new_claude_pane $WS_CR C:/temp/restore-WS-claude-renamed will-rename)
dc pane rename 'MY-CUSTOM-TITLE' --pane $P_CR > /dev/null 2>&1

echo "  ws_ids: empty=$WS_E shell-s=$WS_SS shell-t=$WS_ST cl-s=$WS_CS cl-t=$WS_CT cl-r=$WS_CR"
echo "  pane_ids: $P_SS $P_ST $P_CS $P_CT $P_CR"

echo "== wait 25s for claude bootstrap + initial task response"
sleep 25

echo "== assert text actually arrived in panes BEFORE kill"
shell_out=$(dc get-output --pane $P_ST --lines 30 2>&1)
if echo "$shell_out" | grep -q "MARKER_SHELL_BEFORE_KILL"; then
  echo "  OK shell-with-text contains MARKER_SHELL_BEFORE_KILL"
else
  echo "  FAIL shell-with-text missing MARKER_SHELL_BEFORE_KILL"
  echo "$shell_out" | tail -5
fi

claude_out=$(dc get-output --pane $P_CT --lines 30 2>&1)
if echo "$claude_out" | grep -q "PINEAPPLEMARKER"; then
  echo "  OK claude-with-text response contains PINEAPPLEMARKER"
else
  echo "  FAIL claude-with-text response missing PINEAPPLEMARKER (claude may not have responded yet)"
  echo "$claude_out" | tail -8
fi

echo "== wait extra 10s for autosave"
sleep 10
PRE=$(snapshot session-restore-pre)
echo "  pre snapshot: $PRE"

echo "== HARD KILL (taskkill /F or kill -9 — Windows shutdown simulator)"
dev_kill_hard
assert_atomic_save_clean || exit 1

echo "== restart dev"
dev_start_bg
dev_wait_ready 180 || exit 1
sleep 5
POST=$(snapshot session-restore-post)
echo "  post snapshot: $POST"

echo
echo "== METADATA DIFF"
if [ "$CHECK_MODE" -eq 1 ]; then
  echo "  skipped in --check mode; baseline summary applies documented tolerance"
  DIFF_EXIT=0
else
  diff_snapshots "$PRE" "$POST"
  DIFF_EXIT=$?
fi

echo
echo "== BASELINE SUMMARY"
SUMMARY_PATH="$EVAL_LIB_OUT_BASE/session-restore-latest-summary.json"
compare_against_baseline "$PRE" "$POST" "$BASELINE_PATH" "$SUMMARY_PATH" "$CHECK_MODE"
CHECK_EXIT=$?

echo
echo "== UI-mount step is MANUAL: open dev window, click each WS-* tab,"
echo "   then verify silent-claude / claude-with-text / claude-renamed all"
echo "   show claude UI (not bare powershell), and silent-shell /"
echo "   shell-with-text show plain powershell (no accidental claude)."

if [ "$CHECK_MODE" -eq 1 ]; then
  exit $CHECK_EXIT
fi
exit $DIFF_EXIT
