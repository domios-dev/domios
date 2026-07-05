---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_3e3fb620323645c4a0f29fd0bb803f8c
handle: first-run-general-agent-selection-screen-with-install-fallback-ms_3e3fb620323645c4a0f29fd0bb803f8c
title: status_update
---

IN_REVIEW: STATUS: in_review
CHANGES: src/components/AgentPicker/* (shared AgentProviderPicker + FirstRunGeneralPicker + preselect/availability tests), src/components/Terminal/TabContent.tsx, src/lib/{types,bridge}.ts, src/hooks/useConfig.ts, src-tauri/src/service/general_agent/{first_run.rs,workspace.rs,mod.rs,watchdog.rs}, src-tauri/src/startup/{states.rs,servers.rs}, src-tauri/src/{commands/agent.rs,lib.rs}, src-tauri/src/api/{agent_routes.rs,routes.rs}, src-tauri/src/config/schema/mod.rs — sha 2b62f51a
BUILD/TYPECHECK/TESTS: no cargo/build (orchestrator gate). npx tsx src/components/AgentPicker/preselectProvider.test.ts PASS; npx tsx src/components/AgentPicker/constants.test.ts PASS; npx tsc --noEmit — no errors in changed src/ files (pre-existing plugin-sdk TS2307 elsewhere). git diff --check clean.
FOLLOWUP: Orchestrator should build/test on warm checkout, then verify fresh profile on dev:
  CODETERM_INSTANCE=firstrun-test CODETERM_API_PORT=<unique> ./build.sh dev
  # walk through: (1) claude installed → preselected confirm, (2) only codex installed, (3) none installed → install cards + re-check.
  # After confirm: protected Main tab spawns, first-run tab closes, GA auto-starts. Screenshot all three states for acceptance.
