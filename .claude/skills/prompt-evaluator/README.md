# prompt-evaluator

Maintainer skill for testing CodeTerm prompt files (system prompts, suffixes,
rules, agent definitions) without manually spawning agents.

This is a **Claude Code skill**, not a CodeTerm feature. It lives in
`.claude/skills/` of the codeterm repo and is loaded automatically by Claude
Code (or any other agent that respects `.claude/skills/`) when working in
this project. It is **not** bundled into the CodeTerm binary.

## TL;DR

```bash
# Single prompt — does it pass its eval set?
python .claude/skills/prompt-evaluator/runner.py \
  --prompt-file src-tauri/prompts/general_agent.md \
  --eval-set evals/general-agent/evals.json \
  --runs 3

# A/B compare with the previous git revision
python .claude/skills/prompt-evaluator/runner.py \
  --prompt-a HEAD~1:src-tauri/prompts/general_agent.md \
  --prompt-b src-tauri/prompts/general_agent.md \
  --eval-set evals/general-agent/evals.json \
  --runs 3 \
  --verbose
```

Exit code 0 = all good, 1 = failures (single mode) or regressions (A/B mode).

## Files

- `SKILL.md` — full skill description with triggers, methodology, tips for writing good evals
- `runner.py` — orchestration: spawns `claude` CLI per eval, collects results, renders markdown report
- `matcher.py` — hybrid expectation matcher (substring + LLM-as-judge)
- `reports/` — output directory (gitignored)

## Requirements

- `claude` CLI in `$PATH` (you have it if you're using Claude Code)
- Python 3.11+
- Git (for `<rev>:<path>` references)
- An Anthropic API key configured for the `claude` CLI (it uses your existing auth)

No pip dependencies — stdlib only.

## Read SKILL.md for details

The `SKILL.md` file in this directory explains the design rationale, how to
read reports, how to write good eval sets, and known limitations. It's the
canonical reference for both human maintainers and agents working with this
harness.
