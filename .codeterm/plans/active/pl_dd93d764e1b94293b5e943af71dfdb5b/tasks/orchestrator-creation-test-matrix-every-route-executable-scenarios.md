---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_1800fa9977ea418094bb4ab291b2e3f2
handle: orchestrator-creation-test-matrix-every-route-executable-scenarios
title: 'Orchestrator creation test matrix: every route, executable scenarios'
status: done
---

Matrix hard gate rerun on dev build 6482750c after creation hardening: PASS=19, FAIL=0, FINDING=5, GATED=2. Local fixes landed in 21cacbdb: CLI refuses agent spawn --role Orchestrator without --workspace/--parent; matrix uses chat-raw/start for pending_greeting/onboarding oracle; H2 uses compact JSON via curl --data-binary; Planner role_profile spawn now sends planner intro as actual task text even when pair spawn has no explicit task body. Gates: matrix run PASS no hard failures; TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::orchestrator::tests PASS 24/24; CODETERM_API_PORT=7686 ./build.sh cli PASS; git diff --check PASS; check-file-sizes PASS; pre-push cargo check/fmt/clippy PASS. Remaining findings are tracked: real plan id placeholder for generic role-spawn, RPC body not echoing mandate, planner intro transcript timing, planner_unpaired doctor ladder, timer-gated nudge visibility.
