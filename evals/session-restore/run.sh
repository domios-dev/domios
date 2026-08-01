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
  WS-* scenario workspace/tab metadata is exact.
  General workspace tab/tab counts may vary by ±1 because the protected
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

def tabs_by_workspace(tabs):
    grouped = {}
    for tab in tabs:
        grouped.setdefault(tab.get("workspace_name"), []).append(tab)
    return grouped

def slim_tab(tab):
    return {
        "title": tab.get("title"),
        "agent_type": tab.get("agent_type"),
        "cwd": tab.get("cwd"),
        "tab_title": tab.get("tab_title"),
        "title_level": tab.get("title_level"),
    }

def read_optional(path, name):
    full = os.path.join(path, name)
    if not os.path.exists(full):
        return {}
    with open(full, "r", encoding="utf-8") as f:
        return json.load(f)

pre_ws = read(pre, "ws.json")
post_ws = read(post, "ws.json")
pre_tabs = read(pre, "tabs.json")
post_tabs = read(post, "tabs.json")
pre_ga_session = read_optional(pre, "ga-session.json")
post_ga_session = read_optional(post, "ga-session.json")
pre_ws_by_name = index_ws(pre_ws)
post_ws_by_name = index_ws(post_ws)
pre_tabs_by_ws = tabs_by_workspace(pre_tabs)
post_tabs_by_ws = tabs_by_workspace(post_tabs)

scenario_names = sorted(name for name in pre_ws_by_name if name.startswith("WS-"))

summary = {
    "workspace_count": {"pre": len(pre_ws), "post": len(post_ws)},
    "tab_count": {"pre": len(pre_tabs), "post": len(post_tabs)},
    "scenarios": {},
    "general": {},
}

for name in scenario_names:
    before = pre_ws_by_name[name]
    after = post_ws_by_name.get(name)
    before_tabs = sorted(
        (slim_tab(p) for p in pre_tabs_by_ws.get(name, [])),
        key=lambda p: (p.get("title") or "", p.get("cwd") or ""),
    )
    after_tabs = sorted(
        (slim_tab(p) for p in post_tabs_by_ws.get(name, [])),
        key=lambda p: (p.get("title") or "", p.get("cwd") or ""),
    )
    summary["scenarios"][name] = {
        "cwd": before.get("cwd"),
        "pre_tab_count": before.get("tab_count"),
        "post_tab_count": after.get("tab_count") if after else None,
        "pre_tab_count": len(before_tabs),
        "post_tab_count": len(after_tabs),
        "pre_tabs": before_tabs,
        "post_tabs": after_tabs,
    }

general_pre = pre_ws_by_name.get("General")
general_post = post_ws_by_name.get("General")
if general_pre and general_post:
    pre_sid = pre_ga_session.get("session_id")
    post_sid = post_ga_session.get("session_id")
    summary["general"] = {
        "pre_tab_count": general_pre.get("tab_count"),
        "post_tab_count": general_post.get("tab_count"),
        "pre_tab_count": len(pre_tabs_by_ws.get("General", [])),
        "post_tab_count": len(post_tabs_by_ws.get("General", [])),
        "session_id_present_pre": bool(pre_sid),
        "session_id_matches_after_sid_wipe": bool(pre_sid) and post_sid == pre_sid,
        "decoy_worker_session_seeded": bool(pre_ga_session.get("decoy_session_id")),
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
print(f"  tabs:      pre={summary['tab_count']['pre']} post={summary['tab_count']['post']}")

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
general_tab_tol = int(tolerances.get("general_tab_count_abs", 0))

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
    for key in ("cwd", "pre_tab_count", "post_tab_count", "pre_tab_count", "post_tab_count", "pre_tabs", "post_tabs"):
        if got.get(key) != want.get(key):
            failures.append(f"{name}.{key}: {got.get(key)!r} != {want.get(key)!r}")

for key in ("pre_tab_count", "post_tab_count"):
    got = summary["general"].get(key)
    want = expected["general"].get(key)
    if got is None or abs(got - want) > general_tab_tol:
        failures.append(f"General.{key}: {got} outside {want} ±{general_tab_tol}")

for key in ("pre_tab_count", "post_tab_count"):
    got = summary["general"].get(key)
    want = expected["general"].get(key)
    if got is None or abs(got - want) > general_tab_tol:
        failures.append(f"General.{key}: {got} outside {want} ±{general_tab_tol}")

for key in ("session_id_present_pre", "session_id_matches_after_sid_wipe", "decoy_worker_session_seeded"):
    got = summary["general"].get(key)
    want = expected["general"].get(key)
    if got != want:
        failures.append(f"General.{key}: {got!r} != {want!r}")

print(f"baseline: {baseline_path}")
print(
    "tolerance: WS-* exact; "
    f"General tabs ±{general_tab_tol}, General tabs ±{general_tab_tol}"
)

if failures:
    print("FAIL: session-restore regression beyond tolerance")
    for failure in failures:
        print(f"  - {failure}")
    sys.exit(1)

print("PASS: session-restore baseline check within tolerance")
PY
}

capture_general_session() {
  local out_path="${1:?usage: capture_general_session <out_json>}"
  local tabs_json
  tabs_json="$(mktemp)"
  dev_get /api/tabs > "$tabs_json"
  PYTHONIOENCODING=utf-8 python - "$tabs_json" "$out_path" <<'PY'
import json
import sys

tabs_path, out_path = sys.argv[1:3]
with open(tabs_path, "r", encoding="utf-8") as f:
    tabs = json.load(f)
tab = next(
    (
        p for p in tabs
        if p.get("workspace_name") == "General"
        and (p.get("title") == "Main" or p.get("tab_title") == "Main")
    ),
    None,
)
payload = {
    "tab_id": tab.get("id") if tab else None,
    "cwd": tab.get("cwd") if tab else None,
    "session_id": tab.get("agent_session_id") if tab else None,
}
with open(out_path, "w", encoding="utf-8") as f:
    json.dump(payload, f, indent=2, sort_keys=True)
    f.write("\n")
if not payload["session_id"]:
    print("ERROR: General tab has no agent_session_id", file=sys.stderr)
    sys.exit(1)
print(payload["session_id"])
PY
  local rc=$?
  rm -f "$tabs_json"
  return $rc
}

seed_general_decoy_worker_session() {
  local ga_json="${1:?usage: seed_general_decoy_worker_session <ga_json> <out_json>}"
  local out_json="${2:?usage: seed_general_decoy_worker_session <ga_json> <out_json>}"
  PYTHONIOENCODING=utf-8 python - "$ga_json" "$out_json" <<'PY'
import json
import os
import pathlib
import sys
import time
import uuid

ga_json, out_json = sys.argv[1:3]
with open(ga_json, "r", encoding="utf-8") as f:
    ga = json.load(f)
cwd = ga["cwd"]
canon = str(pathlib.Path(cwd).resolve()) if os.path.exists(cwd) else cwd
encoded = "".join(ch if ch.isalnum() else "-" for ch in canon)
projects = pathlib.Path.home() / ".claude" / "projects" / encoded
projects.mkdir(parents=True, exist_ok=True)
sid = str(uuid.uuid4())
path = projects / f"{sid}.jsonl"
line = {
    "type": "user",
    "cwd": cwd,
    "message": {
        "content": "codeterm agent report haiku facts done\nGPU facts\nworker transcript decoy"
    },
}
path.write_text(json.dumps(line) + "\n", encoding="utf-8")
future = time.time() + 5
os.utime(path, (future, future))
ga["decoy_session_id"] = sid
ga["decoy_session_path"] = str(path)
with open(out_json, "w", encoding="utf-8") as f:
    json.dump(ga, f, indent=2, sort_keys=True)
    f.write("\n")
print(f"{sid} {path}")
PY
}

strip_general_sid_from_last_json() {
  PYTHONIOENCODING=utf-8 python <<'PY'
import json
import os
import sys

path = os.path.join(os.path.expanduser("~"), ".codeterm-dev", "sessions", "last.json")
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

removed = []

def strip_leaf(node):
    if not isinstance(node, dict):
        return
    if node.get("title") == "Main" and node.get("agent_type") == "claude":
        for key in ("agent_session_id", "claude_session_id"):
            if node.pop(key, None):
                removed.append(key)
    if node.get("type") in ("Split", "Leaf"):
        strip_leaf(node.get("first"))
        strip_leaf(node.get("second"))

for ws in data.get("workspaces", []):
    if ws.get("name") != "General":
        continue
    for tab in ws.get("tabs", []):
        strip_leaf(tab.get("split_tree"))

if not removed:
    print("ERROR: no General session id found in last.json to strip", file=sys.stderr)
    sys.exit(1)

tmp = path + ".tmp"
with open(tmp, "w", encoding="utf-8") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
os.replace(tmp, path)
print("  stripped General session id from last.json: " + ",".join(sorted(set(removed))))
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

P_SS=$(new_shell_tab $WS_SS C:/temp/restore-WS-shell-silent silent-shell)

P_ST=$(new_shell_tab $WS_ST C:/temp/restore-WS-shell-with-text shell-with-text)
# pwsh PTY linked != shell-ready-for-input. await_pty_link returns
# as soon as the PTY is registered, but the shell process inside
# (powershell.exe) takes another 5-10s to render its prompt and
# attach stdin. exec before that point gets bytes-eaten silently.
# 12s is the empirical safe window on Win11/PS5.
sleep 12 && dc tab exec --tab $P_ST 'echo MARKER_SHELL_BEFORE_KILL' > /dev/null 2>&1

P_CS=$(new_claude_tab $WS_CS C:/temp/restore-WS-claude-silent silent-claude)

P_CT=$(new_claude_tab $WS_CT C:/temp/restore-WS-claude-with-text claude-with-text 'reply with single word PINEAPPLEMARKER')

P_CR=$(new_claude_tab $WS_CR C:/temp/restore-WS-claude-renamed will-rename)
dc tab rename 'MY-CUSTOM-TITLE' --tab $P_CR > /dev/null 2>&1

echo "  ws_ids: empty=$WS_E shell-s=$WS_SS shell-t=$WS_ST cl-s=$WS_CS cl-t=$WS_CT cl-r=$WS_CR"
echo "  tab_ids: $P_SS $P_ST $P_CS $P_CT $P_CR"

echo "== wait 25s for claude bootstrap + initial task response"
sleep 25

echo "== assert text actually arrived in tabs BEFORE kill"
shell_out=$(dc tab get-text --tab $P_ST 2>&1)
if echo "$shell_out" | grep -q "MARKER_SHELL_BEFORE_KILL"; then
  echo "  OK shell-with-text contains MARKER_SHELL_BEFORE_KILL"
else
  echo "  FAIL shell-with-text missing MARKER_SHELL_BEFORE_KILL"
  echo "$shell_out" | tail -5
  exit 1
fi

claude_out=$(dc tab get-text --tab $P_CT 2>&1)
if echo "$claude_out" | grep -q "PINEAPPLEMARKER"; then
  echo "  OK claude-with-text response contains PINEAPPLEMARKER"
else
  echo "  FAIL claude-with-text response missing PINEAPPLEMARKER (claude may not have responded yet)"
  echo "$claude_out" | tail -8
  exit 1
fi

echo "== wait extra 10s for autosave"
sleep 10
PRE=$(snapshot session-restore-pre)
echo "  pre snapshot: $PRE"
echo "== seed newer worker decoy in General Agent cwd"
GA_PRE_TMP="$EVAL_LIB_OUT_BASE/session-restore-ga-pre.tmp.json"
GA_PRE_SID=$(capture_general_session "$GA_PRE_TMP") || exit 1
seed_general_decoy_worker_session "$GA_PRE_TMP" "$PRE/ga-session.json" || exit 1
echo "  General session before restart: $GA_PRE_SID"

echo "== HARD KILL (taskkill /F or kill -9 — Windows shutdown simulator)"
dev_kill_hard
assert_atomic_save_clean || exit 1
strip_general_sid_from_last_json || exit 1

echo "== restart dev"
dev_start_bg
dev_wait_ready 180 || exit 1
sleep 5
POST=$(snapshot session-restore-post)
echo "  post snapshot: $POST"
GA_POST_SID=$(capture_general_session "$POST/ga-session.json") || exit 1
if [ "$GA_POST_SID" = "$GA_PRE_SID" ]; then
  echo "  OK General Agent resumed its own session after sid wipe + newer decoy"
else
  echo "  FAIL General Agent session drifted: pre=$GA_PRE_SID post=$GA_POST_SID"
  exit 1
fi

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
