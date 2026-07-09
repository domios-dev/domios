---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_066fd227638f4edbbf65c68dfc9f9922
handle: agent-message
title: 'CLI: immediate agent message to running worker pane'
status: done
tags:
- cli
- ergonomics
---

# CLI: immediate orchestrator→worker message (`codeterm agent message <pane>`)

FRICTION HIT LIVE: an orchestrator cannot send a mid-flight follow-up prompt to a running worker pane.
- `SendMessage` (teammate-inbox) does NOT reach a CodeTerm agent pane — wrong channel, silently dropped (Andrey caught it: agents sat blocked "no task").
- Only delivery paths today: respawn with `--task-file`, or the `codeterm delayed create --pane X --delay 1s` reminder hack.

NEED: a first-class immediate prompt injection, e.g. `codeterm agent message <pane> --text/--file` (or `pane send`), that types the text into the target agent pane as a user turn — the same path `delayed` uses, minus the timer. Lets the orchestrator steer/redirect/refine a worker without killing+respawning it (cheaper, preserves context) — core to the loop/pass model.

Acceptance: `codeterm agent message <pane> --file brief.md` delivers into a running claude/codex pane and the agent acts on it; bench it against the §34 respawn case.
