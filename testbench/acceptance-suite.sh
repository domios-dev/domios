#!/usr/bin/env bash
# Minimal acceptance command set for agent-state detection (runbook rev 5).
# Proves, with numbers: final Idle for normal answers, Question-banner-only for
# real terminal menus, zero inline cards, and Codex transcript ordering.
#
# Layering (READ THIS):
#   * production classifier == src-tauri/.../offline_state_classifier.rs
#   * render_timeline.rs runs that classifier and BAKES detector.state into
#     timeline.json. The python readers below read the BAKED field — so after
#     ANY Rust classifier change you MUST regenerate timelines (--regen) or the
#     numbers are stale.
#   * state_doctor.rs / state_calibration.rs compile the classifier directly
#     (no baked field) — they reflect current source WITHOUT regeneration.
#
# Usage:
#   testbench/acceptance-suite.sh            # gate over committed timelines
#   testbench/acceptance-suite.sh --regen    # rebuild render_timeline + re-render all casts first
#   testbench/acceptance-suite.sh --live     # also run live state_doctor (compiles classifier)
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
REGEN=0; LIVE=0
for a in "$@"; do case "$a" in --regen) REGEN=1;; --live) LIVE=1;; esac; done

echo "================ BUILD IDENTITY ================"
GIT_HEAD="$(git rev-parse --short HEAD)"
DEV_BUILD="$(curl -s http://127.0.0.1:7686/api/health 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("build","?"))' 2>/dev/null || echo unreachable)"
echo "git HEAD     : $GIT_HEAD"
echo "dev daemon   : $DEV_BUILD  $( [ "$DEV_BUILD" = "$GIT_HEAD" ] && echo '(MATCHES HEAD)' || echo '(STALE — restart dev before trusting LIVE/API state)')"

if [ "$REGEN" = 1 ]; then
  echo "================ REGENERATE TIMELINES (after classifier change) ================"
  cargo build --manifest-path src-tauri/Cargo.toml --example render_timeline 2>&1 | tail -2
  RT="src-tauri/target/debug/examples/render_timeline"
  n=0
  while IFS= read -r cast; do
    d="$(dirname "$cast")"; out="$d/timeline.json"
    [ -f "$d/meta.json" ] && meta="--meta $d/meta.json" || meta=""
    [ -f "$d/phases.json" ] && phases="--phases $d/phases.json" || phases=""
    # Synthetic casts have no provider plugin; their band is declared, not defaulted.
    rows="$(python3 testbench/provider_rows.py "$d" --fixture-rows 0)" || continue
    # shellcheck disable=SC2086
    "$RT" "$cast" "$out" --status-bar-rows "$rows" $meta $phases >/dev/null 2>&1 && n=$((n+1))
  done < <(find testbench/replays -name raw.cast)
  echo "regenerated $n timelines from raw.cast"
fi

echo "================ OFFLINE ACCEPTANCE GATE (baked detector.state) ================"
python3 testbench/acceptance-gate.py
GATE=$?

echo "================ DISPLAY HYGIENE GATE (baked detector text) ================"
python3 testbench/status_hygiene.py --self-test >/dev/null
HYGIENE_SELF=$?
python3 testbench/status_hygiene.py --corpus testbench/replays --quiet
HYGIENE=$?

echo "================ CODEX TRANSCRIPT ORDERING (data-layer proxy for P0 #2) ================"
# DOM oracle is Windows/WebView2-only (tests/playwright/README.md). On macOS we
# check the transcript the ChatUI renders: /api/tabs/{id}/chat must be seq- and
# timestamp-monotonic. Clean here + visible disorder == pure frontend render bug.
source evals/_lib/lib.sh 2>/dev/null
dev_get /api/tabs 2>/dev/null | python3 -c '
import sys,json
d=json.load(sys.stdin)
codex=[p["id"] for p in d if p.get("agent_type")=="codex"]
print("codex tabs:", codex or "none")
' 2>/dev/null
for pid in $(dev_get /api/tabs 2>/dev/null | python3 -c 'import sys,json;print(" ".join(p["id"] for p in json.load(sys.stdin) if p.get("agent_type")=="codex"))' 2>/dev/null); do
  dev_get "/api/tabs/$pid/chat" 2>/dev/null | python3 -c "
import sys,json
try: d=json.load(sys.stdin)
except Exception: print('  $pid: no transcript'); raise SystemExit
m=d.get('messages',[])
ti=si=0; pt=ps=None
for x in m:
    t=x.get('timestamp'); s=x.get('seq')
    if pt and t and t<pt: ti+=1
    if ps is not None and s is not None and s<ps: si+=1
    pt,ps=t,s
print(f'  $pid: n={len(m)} ts_inversions={ti} seq_inversions={si}  {\"ORDERED\" if ti==si==0 else \"OUT OF ORDER\"}')
"
done
echo "  NOTE: ChatUI DOM order NOT checked — Playwright/CDP is Windows/WebView2 only (this host is macOS)."

echo "================ RESULT ================"
[ "$GATE" = 0 ] && echo "ACCEPTANCE GATE: GREEN" || echo "ACCEPTANCE GATE: RED"
[ "$HYGIENE_SELF" = 0 ] && [ "$HYGIENE" = 0 ] && echo "DISPLAY HYGIENE: GREEN" || echo "DISPLAY HYGIENE: RED"
if [ "$GATE" = 0 ] && [ "$HYGIENE_SELF" = 0 ] && [ "$HYGIENE" = 0 ]; then
  exit 0
fi
exit 1
