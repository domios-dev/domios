---
name: prompt-evaluator
description: Evaluate, A/B compare, or check for regression in any CodeTerm prompt file (general_agent.md, worker_suffix.md, codeterm_rule.md, agent role definitions in codeterm mem, bridge rules, etc). Triggers when the user asks to "test the prompt", "run evals", "check for regression", "did my prompt change break anything", or "compare prompt before and after". Runs runner.py against the matching eval set under evals.
---

# prompt-evaluator

This is a pointer, not a copy. The skill itself lives at `.agents/skills/prompt-evaluator/SKILL.md`; read that file and follow it.

Everything the skill needs — scripts, helpers, references — lives beside it under `.agents/skills/prompt-evaluator/`. Never edit this pointer to carry procedure, and never copy the skill here: one source, no drift.
