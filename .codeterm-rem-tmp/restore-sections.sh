#!/usr/bin/env bash
# Fire-and-poll section restore into pl_as7ty5 (daemon RPC responses hang; writes commit).
set -u
cd "$(dirname "$0")/.."
R=.codeterm-rem-tmp/pl_ghk358-recovery.md
MAP=.codeterm-rem-tmp/idmap-as7.txt
SPEC=sp_o6l6of
PLAN=pl_as7ty5
SPEC_YAML=.codeterm/plans/active/pl_as7ty5/spec.yaml
PLAN_YAML=.codeterm/plans/active/pl_as7ty5/plan.yaml
SPEC_PARENT=sec_46l3he
PLAN_PARENT=""

poll_id() { # $1 yaml file, $2 exact title
  local i id
  for i in $(seq 1 30); do
    id=$(grep -B1 -F "title: $2" "$1" 2>/dev/null | grep -o 'sec_[a-z0-9]*' | head -1)
    [ -n "$id" ] && { echo "$id"; return 0; }
    sleep 1
  done
  return 1
}

add_spec() { # $1 old-id, $2 title, $3 start, $4 end
  if grep -qF "title: $2" "$SPEC_YAML"; then :; else
    timeout 8 codeterm spec section add "$SPEC" --parent "$SPEC_PARENT" --title "$2" \
      --body "$(awk "/^### $3/{f=1;next} /^### $4|^## 3\\./{f=0} f" $R)" >/dev/null 2>&1
  fi
  local id; id=$(poll_id "$SPEC_YAML" "$2") || { echo "FAIL $1"; return 1; }
  echo "$1=$id" >> "$MAP"; echo "$1=$id"
}

add_plan() { # $1 old-id, $2 title, $3 start, $4 end
  if grep -qF "title: $2" "$PLAN_YAML"; then :; else
    timeout 8 codeterm plan section add "$PLAN" --parent "$PLAN_PARENT" --title "$2" \
      --body "$(awk "/^### $3/{f=1;next} /^### $4|^## 4\\./{f=0} f" $R)" >/dev/null 2>&1
  fi
  local id; id=$(poll_id "$PLAN_YAML" "$2") || { echo "FAIL $1"; return 1; }
  echo "$1=$id" >> "$MAP"; echo "$1=$id"
}

# --- spec children under sec_46l3he ---
add_spec sec_l4uzrj "Problem & audit baseline" "sec_l4uzrj" "sec_zb6lvx"
add_spec sec_zb6lvx "Success criteria & correctness vector" "sec_zb6lvx" "sec_btbfo0"
add_spec sec_btbfo0 "Open gaps & decisions needed (PTY)" "sec_btbfo0" "sec_ldl1vu"
add_spec sec_ldl1vu "Issue-triage scope: WHAT & WHY" "sec_ldl1vu" "sec_gnk2xm"
add_spec sec_gnk2xm "Live defect: Enter not submitted after paste (chat UI / cross-pane)" "sec_gnk2xm" "sec_sve813"
add_spec sec_sve813 "#240 P0: cross-mesh delivery violates the guarantee" "sec_sve813" "sec_q2hqim"
add_spec sec_q2hqim "Chat UI agent-binding regression (research-first)" "sec_q2hqim" "sec_6ng8bd"
add_spec sec_6ng8bd "Andrey-directed architecture refactor lanes" "sec_6ng8bd" "sec_0gi9oc"
add_spec sec_0gi9oc "Acceptance model: bytes-written vs provider-accepted" "sec_0gi9oc" "NEVERMATCH"

# --- plan parent ---
if ! grep -qF "title: PTY delivery guarantee — recovered phases (pl_ghk358)" "$PLAN_YAML"; then
  timeout 8 codeterm plan section add "$PLAN" --title "PTY delivery guarantee — recovered phases (pl_ghk358)" \
    --body "Parent for the 14 execution phases recovered from lost plan pl_ghk358. Provenance: recovery export sha256 78a67aaf79b07838e39251ebaff501a63bc3321b6f1ebc8cc1085a7a85ef2e07. Original ordering and gating preserved. No task under this parent closes from report volume — evidence only." >/dev/null 2>&1
fi
PLAN_PARENT=$(poll_id "$PLAN_YAML" "PTY delivery guarantee — recovered phases (pl_ghk358)") || { echo "FAIL PLAN_PARENT"; exit 1; }
echo "PLAN_PARENT=$PLAN_PARENT" >> "$MAP"; echo "PLAN_PARENT=$PLAN_PARENT"

# --- plan phases under parent ---
add_plan sec_yhc5x1 "Phase 1 — Land & verify in-flight PTY delivery fix" "sec_yhc5x1" "sec_7oj4py"
add_plan sec_7oj4py "Phase 2 — PTY audit consolidation & evidence hardening" "sec_7oj4py" "sec_l7vkwa"
add_plan sec_l7vkwa "Phase 3 — Issue-triage intake protocol" "sec_l7vkwa" "sec_a3w0yh"
add_plan sec_a3w0yh "Phase 4 — Crash & data-integrity issues (#235 #237 #236 #239)" "sec_a3w0yh" "sec_7kbewk"
add_plan sec_7kbewk "Phase 5 — Platform & release issues (#203 #209)" "sec_7kbewk" "sec_jop3mw"
add_plan sec_jop3mw "Phase 6 — Planner/CLI DX & orchestration issues (#230 #231 #232)" "sec_jop3mw" "sec_asykd3"
add_plan sec_asykd3 "Phase 7 — Small CLI/bridge hygiene issues (#233 #234 #238)" "sec_asykd3" "sec_ewo6v2"
add_plan sec_ewo6v2 "Phase 8 — Frontend issue (#185)" "sec_ewo6v2" "sec_190dqf"
add_plan sec_190dqf "Phase 9 — Cron/skill-loading reliability (#179)" "sec_190dqf" "sec_ta6h8v"
add_plan sec_ta6h8v "Phase 10 — Closure gate" "sec_ta6h8v" "sec_pgcm75"
add_plan sec_pgcm75 "Phase 1b — Mesh delivery integrity (#240, P0)" "sec_pgcm75" "sec_oyfez3"
add_plan sec_oyfez3 "Phase 11 — Chat UI agent-binding regression (research-first)" "sec_oyfez3" "sec_sy0gh3"
add_plan sec_sy0gh3 "Phase 12 — Architecture refactor lane integration" "sec_sy0gh3" "sec_docuvs"
add_plan sec_docuvs "Phase 1c — Provider acceptance observability (P0)" "sec_docuvs" "NEVERMATCH"

echo "DONE"
