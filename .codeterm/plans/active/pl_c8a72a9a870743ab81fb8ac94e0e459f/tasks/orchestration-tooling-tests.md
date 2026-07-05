---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_bd50e3c9ce7c466e821e0f496deae591
handle: orchestration-tooling-tests
title: Tests for heartbeats/CLI-hints/agent-brief+prompt formatters
status: done
---

# Tests for orchestration tooling: heartbeats, CLI hints, agent brief/prompt formatters
Andrey: everything that FORMATS agent-facing text must be tested. Cover with unit tests:
1. HEARTBEAT generation — whatever builds the delayed self-message / reminder payload (content, re-arm cadence, current-focus line).
2. CLI HINT givers (the cli-guardrail hints: "task not linked to section", "no model/why", etc.) — assert the right hint fires for the right state and NOT when satisfied.
3. AGENT TASK-BRIEF formatters — how a task is rendered into a worker brief / `task show` (the worker-facing view), incl. the worker-preamble prepend.
4. ORCHESTRATOR + SUBAGENT PROMPT formatters — the templates that shape orchestrator/worker prompts/hints.
Find where each lives (CLI hints likely in cli/ near plan task add; heartbeat payload; task render). Add focused tests so changing a template can't silently break the agent-facing contract. Note which are pure functions (easy) vs need fixtures.
