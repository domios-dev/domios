---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_d77b2f26e74a4a64bff2475d8f72ede9
handle: gh-185-chat-terminal-switch-sidebar-agent-icons-intermittently-disappear-ms_d77b2f26e74a4a64bff2475d8f72ede9
title: report_refused
---

PARTIAL: REPORT NOT SENT
readiness report refused: this git worktree has uncommitted lane changes.
Next commands:
  git status --short
  git add <files>
  git commit -m "<message>"
  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."
Or report WIP with:
  codeterm agent report --status partial "..."
Dirty lane files:
  plugins/provider-codex/tests/plugin.test.ts
  src-tauri/prompts/bridge_mem.md
  src-tauri/prompts/general_agent.md
  src-tauri/src/prompts.rs
  src/components/MemoryPanel/memoryApi.ts
  src/components/Terminal/PaneSessionChat.tsx
  src/hooks/useAgentProviders.ts
  .codeterm/specs/sp_d2c70550432f4a829c89cf4a4cb7c3c5/sections/active-work.md

Original readiness report that was refused locally:
STATUS: in_review
CHANGES: src/hooks/useAgentProviders.ts (syncAgentDescriptorCache skips setAgentDescriptors while loading=true — prevents secondary useAgentProviders mounts from wiping shared cache); src/hooks/useAgentProviders.test.ts (GH #185 regression, 3 cases)
BUILD/TYPECHECK/TESTS: npx tsx src/hooks/useAgentProviders.test.ts PASS 3/3; npx tsx src/lib/agentDescriptorCache.test.ts PASS 6/6; npx tsc --noEmit PASS; npx playwright test tests/playwright/suites/16-chat-view/chat-picker-default.spec.ts --project=golden PASS 1/1
FOLLOWUP: Root cause confirmed — App + TabContent + Sidebar + Settings each call useAgentProviders(); every mount ran setAgentDescriptors([]) on initial empty items before fetch completed, clearing icon_svg/supports_chat_ui for all consumers (blank sidebar AgentIcon slots + TabContent showSwitch=false hiding Chat|Terminal switch). Fix is minimal loading guard. Dev still on build 89bea411 (pre-fix); orchestrator should restart dev and manually verify 3 scenarios from task (20+ mode switches, settings open/close, dev restart mid-session) before closing #185.
