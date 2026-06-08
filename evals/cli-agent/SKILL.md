---
title: CLI Agent Benchmark
area: testing
tags: [benchmark, cli, agents, integration]
---

# CLI Agent Benchmark

Drive real agents (claude/codex/opencode) in the DEV console (port 7686) through
every CodeTerm CLI command via natural-language asks, then judge their parsed
transcripts. The model is the judge — there is no automated scorer.

## When to use
Verifying that agents understand and correctly execute the CLI, after CLI/doc/
skill changes, or to measure multi-step reliability across providers.

## Run
1. Ensure dev is up and current: `./build.sh dev --wait`.
2. A batch: `bash evals/cli-agent/run-suite.sh claude,codex,opencode evals/cli-agent/cases/mem.yaml`
3. Transcripts land in `evals/cli-agent/runs/<ts>/<case>-<provider>.md` (readable)
   and `.json` (raw normalized messages from `GET /api/panes/:id/chat`).
4. Read each transcript and fill `judge-template.md` → `runs/<ts>/verdict.md`.

## Rubric (per case × provider)
1. **Understood** the ask. 2. **Correct command + flags**. 3. **Skill usage**
(used the `codeterm` skill / `docs`, didn't hallucinate flags). 4. **Step
efficiency** (nudges to success; 0 = one-shot). 5. **Acted, not described** (a
real tool_call + result is in the transcript).

## Bug-fix loop
- Unclear docs → fix `src-tauri/prompts/codeterm_cli.md` (the shared piece
  included into both the general-agent prompt and the seed skill; guarded by
  `prompt_cli_commands_exist`), rebuild, re-run the case.
- Card mis-render → fix `packages/chat-ui`, rebuild web, re-run.
- CLI bug → fix Rust, rebuild, re-run.

## Safety
Never run `safety: describe-only` cases for real (mesh connect, bridge send,
secrets, system autostart, update install, plugin mutate, config set, theme
apply, destructive mem). For those cases the `prompt` asks the agent to *explain*
or dry-run, not execute. All bench workspaces are torn down after each case.

## How the read works
`codeterm pane chat --pane <id> [--since N] --json` (and `GET /api/panes/:id/chat`)
return the already-parsed normalized messages — `type` (user/assistant/thinking/
tool_call/tool_result), `content`, `toolName`, `toolInput`, `toolResult` — the
same shape the chat UI renders. Stateless; no tailer.
