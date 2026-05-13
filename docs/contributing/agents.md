# Agent roles & orchestration

CodeTerm has a 4-level agent hierarchy: **Orchestrator → Lead → Worker → SubWorker**.

**Your role depends on how you were spawned.** Check `$CODETERM_ROLE` env var or look for a role marker in your initial task prompt.

- **No role / not spawned by another agent** → you are the **Orchestrator**. You are the top. You decompose user requests into tasks and delegate to specialized workers.
- **Spawned with `--role Lead`** → you are a **Lead**. You received a bounded sub-task from an Orchestrator. You may further decompose it and spawn Workers (see `src-tauri/prompts/lead_suffix.md`). Report up when done.
- **Spawned with `--role Worker`** → you are a **Worker**. You execute one concrete task. You DO NOT spawn other agents. When done, run `codeterm agent report "..."` and stop.
- **Spawned with `--role SubWorker`** → you are a **SubWorker**. Same rules as Worker, but your parent is a Lead, not the Orchestrator.

## Role skills (in mem, not `.claude/agents/`)

CodeTerm dogfoods its own `codeterm mem` as the single source of truth for role definitions. The `.claude/agents/` folder is intentionally empty — see the "We dogfood..." section in `CLAUDE.md` for the rationale.

| Role | Mem id | Owns | Typical role |
|------|--------|------|--------------|
| Backend dev | `skill_backend_dev_role_rust_src_tauri_for_codeterm` | `src-tauri/` Rust code | Worker (or Lead for multi-module refactors) |
| Frontend dev | `skill_frontend_dev_role_react_typescript_src_for_codeterm` | `src/` React/TS code | Worker (or Lead for multi-module refactors) |
| Docs sync | `skill_docs_sync_role_keep_docs_in_sync_with_code_for_codeterm` | `docs/*.md`, `README.md`, skills | Worker |
| Code reviewer | `skill_code_reviewer_role_read_only_review_for_codeterm` | Read-only review | Worker |
| Prompt evaluator | `skill_prompt_evaluator_role_a_b_and_regression_testing_for_codeterm` | Prompt files + eval sets in `evals/` | Worker (runs `.claude/skills/prompt-evaluator/runner.py`) |
| End-to-end release | `skill_end_to_end_github_release_for_codeterm` | Full release pipeline | Lead (no specialized agent — any Lead can do it) |

`.claude/skills/` (project-local) is kept for skills with **runnable code** (e.g. `prompt-evaluator/runner.py`). Mem holds markdown content only; skills with scripts stay on disk and their `SKILL.md` is the entry point the role-skill in mem points to.

## Orchestrator-only rules

(Read this only if you are the Orchestrator — no role, running at the top.)

Role definitions live in mem. Spawn pattern: spawn a generic claude agent and tell it to `codeterm mem get <role_skill_id>` BEFORE starting the concrete task. The agent reads the skill, internalizes the role, then executes.

### When to delegate vs do-it-yourself

**Delegate** when the task is scoped, parallelizable, or outside your current context:
- "Add these 15 tests to service/pane.rs" → backend-dev
- "Extract this hook from the component" → frontend-dev
- "Review the last 3 commits for issues" → code-reviewer
- "Update docs after these changes" → docs
- "Bump version and release" → no specialized agent; spawn a Lead with the prompt "`codeterm mem get skill_end_to_end_github_release_for_codeterm` and follow it end-to-end. Use ROG for Windows build (mesh peer)."

**Do-it-yourself** when the task is small, architectural, or cross-cutting:
- Single-file edit under ~50 lines
- Architecture decisions (splitting modules, choosing patterns)
- Multi-module refactors spanning 5+ files across both frontend and backend
- Tasks that require holding the full project context

### How to spawn workers

Two mechanisms:

**1. Built-in Agent tool** (in-process, cheap, fast):
```
Agent(subagent_type="general-purpose",
      prompt="codeterm mem get skill_backend_dev_role_rust_src_tauri_for_codeterm — that is your role definition; follow it. TASK: …")
```
Use for: quick research, small isolated tasks, one-off subtasks. The agent runs in your context and returns a message. No pane is created.

**2. `codeterm agent spawn claude`** (heavyweight, visible, dogfooded):
```bash
codeterm agent spawn claude \
  --role Worker --parent $CODETERM_PANE_ID \
  --title "backend-dev: add pane tests" \
  --task "First run: codeterm mem get skill_backend_dev_role_rust_src_tauri_for_codeterm — that is your role and rules, follow them strictly.

TASK: add unit tests to src-tauri/src/service/pane.rs covering …"
```
Use for: bigger parallelizable tasks, when the user should watch work happen, when the worker needs its own PTY and real filesystem access. Reports auto-inject into your pane as `[Report from ...]`.

**Important:** do NOT inline the role-skill body in the task — it wastes tokens on every spawn and decouples from mem updates. Tell the agent to fetch via `codeterm mem get <skill_id>` as the first action. Mem is the source of truth and updates automatically.

### Role assignment when spawning

**Tell the agent its role explicitly via mem fetch in the task prompt.** `codeterm agent spawn --role` sets the backend hierarchy state, but the agent itself doesn't know its role-specific guidance unless you point it at the right mem skill.

Example task prefix:

```
First run: codeterm mem get <role_skill_id> — that is your role definition; follow it.
You are a Worker. Do not spawn other agents. Report via `codeterm agent report` when done.

TASK: ...
```

- **Lead**: spawn with `--role Lead --can-spawn` AND include: *"You are a Lead. First run `codeterm mem get <role_skill_id>` and `cat src-tauri/prompts/lead_suffix.md`. You may spawn workers via `codeterm agent spawn claude --role Worker --parent $CODETERM_PANE_ID --task '...'`. Tell each worker to fetch its role skill from mem too."*
- **Worker**: spawn with `--role Worker --parent $CODETERM_PANE_ID` AND include: *"You are a Worker. First run `codeterm mem get <role_skill_id>`. Do NOT spawn other agents. Finish your task, run `codeterm agent report '...'`, exit."*
- **Default preference**: spawn as Worker. Only promote to Lead when you genuinely need a sub-orchestrator (e.g. "refactor the whole frontend state layer" → frontend-dev role as Lead spawning 4 Worker sub-tasks).

**Rule of thumb:** spawn via `codeterm agent spawn` when the task touches >3 files, needs its own filesystem/PTY context, or the user benefits from watching it run. Otherwise use the built-in Agent tool.

### Reporting back

Workers report via `codeterm agent report "..."`. Reports auto-inject into your terminal. Do NOT poll or sleep — just continue working. When a report arrives, act on it.

Worker report template (enforced via `worker_suffix.md`):
```
STATUS: done | blocked | partial
CHANGES: files touched
BUILD/TYPECHECK/TESTS: result
FOLLOWUP: anything Lead should know
```

### Recursion limits

- Workers (backend-dev, frontend-dev, code-reviewer, docs) MUST NOT spawn other workers. If they need decomposition, they report back and you spawn siblings.
- Release Lead (no specialized type — just spawn with the release skill prompt) coordinates multi-machine builds via mesh and may spawn helpers. Still ultimately reports to you.

### Proactive delegation rules

**After significant changes, auto-invoke the docs sync role.** Significant = new Tauri command, new CLI subcommand, new config option, new feature, API breaking change, README-visible behavior change. Don't ask — just spawn with `codeterm mem get skill_docs_sync_role_keep_docs_in_sync_with_code_for_codeterm` and the task "sync docs with HEAD~N".

**Any edit to a CodeTerm prompt file → auto-invoke the prompt-evaluator role before considering the change done.** This applies to:
- `src-tauri/prompts/*.md` (general_agent, worker_suffix, lead_suffix, orchestrator_suffix, codeterm_rule, bootstrap_instructions, cron_complete_suffix)
- Any future prompt-like artifact that has an eval set under `evals/`

Rules:
- Spawn an agent with `codeterm mem get skill_prompt_evaluator_role_a_b_and_regression_testing_for_codeterm` + task "A/B compare HEAD~1 vs HEAD for <path>. Report regressions." after committing a prompt change locally but before pushing.
- If the evaluator reports `Δ < 0` on any eval, fix or revert the prompt change before pushing. Do not push through a regression.
- If no eval set exists for the prompt file yet, first ask the evaluator to bootstrap one from the prompt's documented behaviors, THEN run the A/B.
- You (as Orchestrator or Lead) do NOT run the prompt-evaluator harness yourself — always delegate. The skill points to `.claude/skills/prompt-evaluator/SKILL.md` + `runner.py` for the actual harness.
- Test cases are added to `evals/<name>/evals.json` by the user or by the evaluator in "Mode 4" (add-a-test-case). You don't hand-edit eval sets.

**Before merging feat branch to develop, auto-invoke the code-reviewer role** on the full branch diff. Act on Critical + High findings before merge.

**Role selection is by artifact, not by task verb.** Rust work → backend-dev skill. React/TS → frontend-dev skill. Docs → docs skill. Reviews → code-reviewer skill. Releases / builds → end-to-end release skill.

### End of Orchestrator-only section.

## Rules for Lead agents

If you are a Lead, you received a bounded sub-task from an Orchestrator. You may:
- Decompose the sub-task further and spawn Workers under yourself (`--role Worker --parent $CODETERM_PANE_ID --can-spawn` is NOT set on Workers).
- Read other panes' output via `codeterm get-output` if relevant.
- Report up via `codeterm agent report "..."` when your whole sub-task is done. Reports from your Workers auto-inject into your pane — do NOT poll or sleep.
- Single structured report template: `STATUS / CHANGES / BUILD / TESTS / FOLLOWUP`.

Do NOT:
- Take on work outside your assigned sub-task.
- Spawn siblings at your own level (only your Orchestrator does that).
- Call docs / code-reviewer proactively — that's the Orchestrator's job. You just report what changed; Orchestrator decides next steps.

## Rules for Worker / SubWorker agents

If you are a Worker or SubWorker, you received exactly ONE concrete task. Do it and stop.

- Do NOT spawn any other agents. No exceptions. If the task needs decomposition, report back to your parent with `STATUS: blocked, FOLLOWUP: needs decomposition`.
- Do NOT read parent's code/docs beyond what's needed for your task.
- Run verification (`./build.sh check`, `cargo test --lib`, `npx tsc --noEmit`) before reporting done.
- Report structured via `codeterm agent report "..."` and exit.

## Dogfooding (applies to everyone)

This project IS CodeTerm. You're running inside CodeTerm. Every spawn via `codeterm agent spawn` is both productive work AND a test of the system. If something feels clunky — the codeterm CLI, the mesh, the reports, the panes — note it in your report. We improve CodeTerm by using it.

## Agent prompts on disk

Prompt templates compiled into the binary via `include_str!()`. Edit `.md` files, recompile to apply.

- `src-tauri/prompts/general_agent.md` — General Agent system prompt (CLAUDE.md template)
- `src-tauri/prompts/bootstrap_instructions.md` — First-run onboarding instructions
- `src-tauri/prompts/worker_suffix.md` — Appended to Worker tasks (report instructions)
- `src-tauri/prompts/lead_suffix.md` — Appended to Lead tasks (spawn + no-sleep)
- `src-tauri/prompts/orchestrator_suffix.md` — Appended to Orchestrator tasks
- `src-tauri/prompts/codeterm_rule.md` — Installed to `~/.claude/rules/codeterm.md`
- `src-tauri/prompts/cron_complete_suffix.md` — Appended to cron tasks (completion instruction)
- `src-tauri/src/prompts.rs` — Rust module that loads all templates

<!-- revision: 1 -->
