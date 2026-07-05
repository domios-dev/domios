---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: task
id: task_e9cd3dadec9145db9137090414b80a47
handle: ui-ux-iter18-workspace-plan-tab-plan-s-as-distinct-in-workspace-entry
title: UI/UX iter18 — workspace plan-tab (plan(s) as distinct in-workspace entry)
status: done
---

WorkspacePlanEntry: map glyph + PLAN pill + title + task-count hint + +N badge for multiple plans; distinct accent/left-bar so it reads apart from session tabs; active (canvas open) highlight; one click -> openWorkspacePlanForEntries opens that plan canvas. workspacePlanEntry +primaryTitle/Hint/ExtraCount, plural tooltip leads w/ freshest. Sidebar untouched (977). Degrades to nothing when daemon doesnt index .codeterm/plans (data dep). Index lacks done_count -> hint=task_count (true done/total pip = backend dep). 0 overflow; tsc+build+cloudFlush 9/9+plan-entry 24/24; all <1000. Committed feat/canvas-perf-polish (unpushed).
