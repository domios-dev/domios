---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_cafa441c807e44da8c43641d9054da0c
handle: release-preparation-everything-staged-nothing-published-user-gate-ms_cafa441c807e44da8c43641d9054da0c
title: decision
---

HANDOFF — exact remaining HUMAN steps (in order, all gated on Andrey): (1) bring codeterm-builder Mac runner online; (2) test the local prod build src-tauri/target/release/bundle/dmg/CodeTerm_1.6.2_aarch64.dmg; (3) on approval, merge release PR #211 into master; (4) git tag release-1.6.2 && git push origin release-1.6.2 (single tag; workflow stamps v1.6.2) — fires all three runners; (5) after publish, verify updater .sig in assets + prod keepawake. HARD STOP holds: no tag, no master merge, no workflow dispatch by the orchestrator.
