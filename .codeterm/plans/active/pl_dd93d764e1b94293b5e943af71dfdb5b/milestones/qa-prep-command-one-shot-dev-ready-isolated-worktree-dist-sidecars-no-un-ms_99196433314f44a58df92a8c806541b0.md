---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_99196433314f44a58df92a8c806541b0
handle: qa-prep-command-one-shot-dev-ready-isolated-worktree-dist-sidecars-no-un-ms_99196433314f44a58df92a8c806541b0
title: status_update
---

STATUS: in_review
CHANGES: scripts/qa-prep.sh (new, 99 lines), build.sh (qa-prep case + dev warm-target detection, +17 lines), docs/LANES.md (rule 3 + revision 2) | SHA: 6f026185 | branch: lane/460103461-qa-prep
VERIFIED: bash -n syntax ok on both scripts/qa-prep.sh and build.sh (did NOT build)
IMPLEMENTATION SUMMARY:
- scripts/qa-prep.sh: 2-stage script (dist via vite build skipping tsc; sidecars copy-from-canonical or build). Group-kill timeout via set -m + background watchdog (mirrors test-capped.sh). Detects canonical checkout via git worktree list --porcelain. Symlinks node_modules from canonical if missing. --timeout flag + QA_PREP_TIMEOUT env override. --force-build-sidecars for explicit rebuild. sidecar_valid() checks >1MB before reusing.
- build.sh: added 'qa-prep' case (delegates to bash scripts/qa-prep.sh ${@:2}). Added 'qa-prep' to usage line. Added warm CARGO_TARGET_DIR auto-detection in 'dev' case: if not already set, detects canonical checkout, exports canonical/src-tauri/target — so ./build.sh dev in any worktree is warm without user action.
- docs/LANES.md: rule 3 extended with one-line dev-start-waits note; revision bumped to 2.
GATES REQUESTED: orchestrator run 'git worktree add /tmp/ct-qa-test lane/460103461-qa-prep && cd /tmp/ct-qa-test && ./build.sh qa-prep --timeout 180' in a fresh worktree; capture 2nd-run timing (should be fast for dist; sidecars copy in <1s from canonical). Verify dist/index.html present and bundled-cli/codeterm + bundled-embed/codeterm-embed-<triple> are >1MB. Then run ./build.sh dev and confirm 'Using shared CARGO_TARGET_DIR' line appears.
FOLLOWUP: If orchestrator wants force-build path tested: set FORCE_BUILD_SIDECARS=1 and verify cargo build falls back with shared CARGO_TARGET_DIR
