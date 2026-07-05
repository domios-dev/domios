---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0bf11c7308d9452d9249bfb3c51afaa7
handle: gh-185-chat-terminal-switch-sidebar-agent-icons-intermittently-disappear-ms_0bf11c7308d9452d9249bfb3c51afaa7
title: status_update
---

PARTIAL: STATUS: partial (fix ready, report blocked on dirty worktree)
CHANGES: src/hooks/useAgentProviders.ts (syncAgentDescriptorCache skips setAgentDescriptors while loading=true); src/hooks/useAgentProviders.test.ts (GH #185 regression, 3 cases)
BUILD/TYPECHECK/TESTS: npx tsx src/hooks/useAgentProviders.test.ts PASS 3/3; npx tsx src/lib/agentDescriptorCache.test.ts PASS 6/6; npx tsc --noEmit PASS; npx playwright chat-picker-default.spec.ts golden PASS 1/1
FOLLOWUP: Root cause — multiple useAgentProviders() mounts (App, TabContent, Sidebar, Settings) each called setAgentDescriptors([]) before fetch completed, wiping shared agentDescriptorCache (blank sidebar icons + showSwitch=false). Fix: loading guard via syncAgentDescriptorCache. Worktree lane/gh-206-ga-skill-lookup-mem has unrelated dirty files blocking in_review report; isolate/commit GH-185 files on a dedicated lane branch, restart dev, verify 3 manual scenarios, then close #185.
