---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_85a8a2397d984a2c8b0322d0b7ce2713
handle: gh-203-windows-startup-spawns-duplicate-shell-conhost-storm
title: 'GH #203: Windows startup spawns duplicate shell/conhost storm'
status: done
---

STEP 1: gh issue view 203 — issue body is the spec. On Windows startup CodeTerm spawns duplicate shells / a conhost storm. Reproduce on ROG via mesh (spawn dev instance, watch process tree at startup: Get-Process conhost,powershell | measure). Suspects: session restore spawning panes twice (restore + fresh default), ConPTY leak per restored pane, or the picker/default-tab creation racing restore. Fix and verify process count matches pane count. If branch work (session-restore fixes) already cured it — close #203 with before/after process counts as evidence.
