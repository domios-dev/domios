---
tags:
- orchestration
- cli
---

# Built-in orchestration doc + autonomy/self-recovery reminders (CLI + heartbeat + plan notifiers)

Andrey: the "what it's like to be an orchestrator" knowledge + the AUTONOMY rule must be reminded to the orchestrator at the convenient moments — from the heartbeat/delay message, from CLI usage, and from plan notifiers. There will be a built-in skill/doc surfaced via CLI.

## Build
1. **`codeterm doc <name>`** (e.g. `codeterm doc orchestration`) — a built-in command that prints the orchestration doc/skill (the live `skill_codeterm_orchestrator` body, or a curated built-in doc). The canonical "if you forgot what it's like to be an orchestrator, read this" entry point. (Today the pointer is `codeterm mem get skill_codeterm_orchestrator`; make it a first-class `codeterm doc orchestration`.)
2. **Heartbeat / delayed-message self-recovery line** (already practiced manually): every re-armed delay carries (a) AUTONOMY — once plan+tasks exist, execute, don't ask; (b) a pointer "forgot how to orchestrate? → `codeterm doc orchestration`". Even the delay reminds you that you act autonomously, including on the delay itself.
3. **CLI guard-hints** (extend the shipped cli-hints guard-rail): on orchestrator-shaped CLI calls — `plan task add`, `plan record`, `agent spawn`, `plan get` — occasionally emit a hint: "you work autonomously; if unsure how to orchestrate, `codeterm doc orchestration`." Cadence-gated so it's not spammy (every N calls / when stale).
4. **Plan notifiers**: when a task is added / flips status / a worker reports, the notifier can carry the orchestrator self-recovery + autonomy reminder at the convenient moment (ties to §36 presence reminders + the post-report action set).

## Why
The orchestrator drifts when running off a degraded heartbeat subset instead of the full skill; and it lapses into asking permission. Surfacing the autonomy rule + the doc pointer at the natural touchpoints (delay, CLI, notifier) keeps the discipline live across context loss. Ties to mem rule_orchestrator_works_autonomously, skill_codeterm_orchestrator, §36 presence, orchestration-skill-postulates.
