#!/usr/bin/env bash
# rev19 acceptance runner — out-of-the-box structured-prompt-server validation +
# regression gates. Run only AFTER Lead's diff is compiled into the running dev
# daemon AND the Adversary C1-C9 review returns GO.
#
# PREREQUISITE (daemon rebuild — picks up BOTH Lead's Rust diff AND the changed
# opencode plugin.js): `./build.sh kill && ./build.sh dev`. Verified safe: build.rs
# emits cargo:rerun-if-changed per plugin file (rerun_if_dir_changed + plugins_root),
# so a normal cargo rebuild re-embeds plugin.js into bundled_plugins — no force-touch
# needed. Confirm http://127.0.0.1:7686/api/health, then run this. Proof is BEHAVIOR
# (--via cli flips FAIL->PASS), not the build hash (Lead's diff is uncommitted).
#
# Two groups:
#   LIVE  (validate the FIX — spawn-path server+subscriber): these prove the fix.
#   OFFLINE (regression — classifier untouched by the spawn diff): must stay green.
#
# B-ORDER (C8): server-check trusts the subscriber only after a confirmed
# --hostname/--port launch. Path C (--via cli) is the out-of-the-box proof; Path A
# (--via rest) is regression (already green).
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
SCRATCH="${SCRATCH:-/private/tmp/rev19}"; mkdir -p "$SCRATCH"
say(){ printf '\n========== %s ==========\n' "$1"; }

say "BUILD IDENTITY"
/usr/bin/curl -s http://127.0.0.1:7686/api/health | python3 -c "import sys,json;d=json.load(sys.stdin);print('daemon build',d['build'])" 2>/dev/null
echo "git HEAD $(git rev-parse --short HEAD)  (Lead diff is uncommitted; build hash can't confirm it — rely on behavior below)"
echo "touched (working tree): $(git status --short src-tauri/src/service src-tauri/src/multiplexer 2>/dev/null | awk '{print $2}' | xargs echo)"

say "LIVE 1/4 — opencode-server-check --via cli  (OUT-OF-THE-BOX proof, C8+C2, B-ORDER)"
./testbench/opencode-server-check.sh --via cli 2>&1 | tail -8 || true

say "LIVE 2/4 — opencode-server-check --via rest (regression, already-green path A)"
./testbench/opencode-server-check.sh --via rest 2>&1 | tail -8 || true

say "LIVE 3/4 — structured-prompt-check (C9 SSE-source-of-truth: state=question via picker)"
./testbench/structured-prompt-check.sh 2>&1 | tail -6 || true

say "LIVE 4/4 — stop-oracle (non-vacuous interrupt). http-abort if server base/session exist"
echo "  run manually with a disposable tab:"
echo "    testbench/stop-oracle.sh --tab <disposable> --mode stop"
echo "    testbench/stop-oracle.sh --tab <disposable> --mode http-abort --oc-base http://127.0.0.1:<port> --oc-session ses_…"

say "OFFLINE regression — regen timelines (classifier untouched by spawn diff)"
( cd src-tauri && cargo build --example render_timeline 2>&1 | tail -1 )
RT=src-tauri/target/debug/examples/render_timeline; rm -rf "$SCRATCH/regen"; mkdir -p "$SCRATCH/regen"
while IFS= read -r c; do d=$(dirname "$c");
  "$RT" "$c" "$SCRATCH/regen/$(echo "$d"|sed 's#testbench/replays/##;s#/#__#g').json" \
    $([ -f "$d/meta.json" ]&&echo --meta "$d/meta.json") $([ -f "$d/phases.json" ]&&echo --phases "$d/phases.json") >/dev/null 2>&1
done < <(find testbench/replays -name raw.cast)
echo "regen done -> $SCRATCH/regen"

say "OFFLINE 1/3 — transition-latency --gate (expect GREEN, 0 safety-net)"
python3 testbench/transition-latency.py "$SCRATCH/regen" --gate >"$SCRATCH/transition-latency.log" 2>&1
TRANSITION=$?
tail -7 "$SCRATCH/transition-latency.log"
say "OFFLINE 2/3 — strobe-metric (watch opencode question<->working)"
python3 testbench/strobe-metric.py "$SCRATCH/regen" --glob '*.json' --gate >"$SCRATCH/strobe-metric.log" 2>&1
STROBE=$?
tail -12 "$SCRATCH/strobe-metric.log"
say "OFFLINE 3/3 — permission-guard-check (synthetic raw-grid; FAIL until classifier guard)"
./testbench/permission-guard-check.sh >"$SCRATCH/permission-guard.log" 2>&1
PERMISSION=$?
tail -5 "$SCRATCH/permission-guard.log"

say "DONE — report before/after vs baselines: cli FAIL->PASS, rest PASS, structured state=question, stop non-vacuous, latency GREEN, permission per guard status"
if [ "$TRANSITION" = 0 ] && [ "$STROBE" = 0 ] && [ "$PERMISSION" = 0 ]; then
  exit 0
fi
exit 1
