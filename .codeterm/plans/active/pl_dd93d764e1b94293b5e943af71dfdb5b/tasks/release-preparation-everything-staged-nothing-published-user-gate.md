---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_cbfaa040fe234e24938747987f928da9
handle: release-preparation-everything-staged-nothing-published-user-gate
title: Release preparation — everything staged, NOTHING published (user gate)
status: in_review
---

GATED ON: develop PR merged. PREPARE the release fully but DO NOT RELEASE — Andrey personally tests the final build before any tag. Steps: (1) release branch off develop; bump package.json version, npm run sync:versions, npm run check:versions, commit 'release: vX.Y.Z'; (2) draft the release PR develop->master with changelog (reuse the develop PR narrative, grouped by area) — OPEN it but DO NOT MERGE; (3) fast precheck parity: cargo check both crates on the branch (the farm runs its own precheck on tag — replicate locally to catch failures early); (4) produce a local production build via ./build.sh build, verify it launches, record the artifact path + sha as a stride — THIS is the build Andrey tests; (5) verify the three-runner farm is ready: runners online (gh api or Actions page), TAURI_SIGNING_PRIVATE_KEY presence per .codeterm/build-machine/README.md — check availability, do not fire anything; (6) write the handoff stride: exact remaining human steps (Andrey: test local prod build -> approve -> merge release PR -> git tag release-X.Y.Z && push). HARD STOP: no tag push, no master merge, no workflow dispatch. Post-release note for the future runbook: verify prod keepawake after the real release (existing recorded requirement).
