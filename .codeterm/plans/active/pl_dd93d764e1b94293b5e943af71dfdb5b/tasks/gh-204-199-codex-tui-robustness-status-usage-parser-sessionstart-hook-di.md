---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_24b2badf369f4ce486262437c48e2256
handle: gh-204-199-codex-tui-robustness-status-usage-parser-sessionstart-hook-di
title: 'GH #204 + #199: Codex TUI robustness — /status usage parser + SessionStart hook diagnosis'
status: done
---

Two codex-provider issues, one lane. STEP 1: gh issue view 204 && gh issue view 199. (A) #204: the /status usage parser breaks across codex TUI format variations — same class as the chat-status fingerprints task (task_3aecfcdf3): parsing must key on STRUCTURAL fingerprints (stable delimiters/shape), never exact copy strings; collect 3+ real /status output fixtures from different codex versions (fixtures directory pattern already exists for provider parsers), lock parser with fixture tests. (B) #199: diagnose why the codex SessionStart hook is disabled/ineffective when pane chat is empty — trace the hook wiring for codex panes, find the emptiness condition, fix or document. If either is already fixed on branch, close that issue with evidence.
