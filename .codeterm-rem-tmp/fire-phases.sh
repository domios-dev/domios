#!/usr/bin/env bash
# Fire all 14 phase adds into pl_as7ty5 under parent sec_47k6nr. RPC hangs; writes commit.
cd "$(dirname "$0")/.."
R=.codeterm-rem-tmp/pl_ghk358-recovery.md
PLAN=pl_as7ty5
PARENT=sec_47k6nr

fire() { # $1 title, $2 start, $3 end
  grep -qF "title: $1" .codeterm/plans/active/pl_as7ty5/plan.yaml && { echo "skip: $1"; return; }
  local body; body=$(awk "/^### $2/{f=1;next} /^### $3|^## 4\\./{f=0} f" $R)
  timeout 12 codeterm plan section add "$PLAN" --parent "$PARENT" --title "$1" --body "$body" >/dev/null 2>&1
  echo "fired: $1"
}

fire "Phase 1 — Land & verify in-flight PTY delivery fix" "sec_yhc5x1" "sec_7oj4py"
fire "Phase 2 — PTY audit consolidation & evidence hardening" "sec_7oj4py" "sec_l7vkwa"
fire "Phase 3 — Issue-triage intake protocol" "sec_l7vkwa" "sec_a3w0yh"
fire "Phase 4 — Crash & data-integrity issues (#235 #237 #236 #239)" "sec_a3w0yh" "sec_7kbewk"
fire "Phase 5 — Platform & release issues (#203 #209)" "sec_7kbewk" "sec_jop3mw"
fire "Phase 6 — Planner/CLI DX & orchestration issues (#230 #231 #232)" "sec_jop3mw" "sec_asykd3"
fire "Phase 7 — Small CLI/bridge hygiene issues (#233 #234 #238)" "sec_asykd3" "sec_ewo6v2"
fire "Phase 8 — Frontend issue (#185)" "sec_ewo6v2" "sec_190dqf"
fire "Phase 9 — Cron/skill-loading reliability (#179)" "sec_190dqf" "sec_ta6h8v"
fire "Phase 10 — Closure gate" "sec_ta6h8v" "sec_pgcm75"
fire "Phase 1b — Mesh delivery integrity (#240, P0)" "sec_pgcm75" "sec_oyfez3"
fire "Phase 11 — Chat UI agent-binding regression (research-first)" "sec_oyfez3" "sec_sy0gh3"
fire "Phase 12 — Architecture refactor lane integration" "sec_sy0gh3" "sec_docuvs"
fire "Phase 1c — Provider acceptance observability (P0)" "sec_docuvs" "NEVERMATCH"
echo "ALL FIRED"
