# Prompts

How CodeTerm ships instructions to LLM agents: where prompt files live, how they are compiled and injected, and how to edit them safely.

## Overview

Every system-level prompt in CodeTerm is a plain `.md` file compiled into the binary via `include_str!()`. There is no runtime prompt store and no hot-reload — **editing a prompt requires a rebuild**. The single loader module is `src-tauri/src/prompts.rs`, which exposes each file as a `pub const &str`.

**Two locations, no third.** Every prompt the project owns lives in one of these:

1. **`src-tauri/prompts/*.md`** — all in-binary prompts. Loaded via `include_str!()` in `src-tauri/src/prompts.rs`. Includes the GA system prompt, role suffixes, framework rules, bridge memory protocol, cron suffixes, AND every memory-worker prompt (extraction, REM dreaming, reranking, worker preamble).
2. **`.claude/agents/*.md`** — Claude Code agent definitions, read from disk on subagent spawn. They cannot move because Claude Code's subagent dispatch hardcodes that path.

If you find a prompt anywhere else, that is a bug — move it into `src-tauri/prompts/` and load it via `prompts.rs`.

Prompts reach the model through five pathways:

1. **System prompt of the General Agent** — written to `~/.codeterm/general/AGENT.md` (and `CLAUDE.md`/`AGENTS.md` wrappers) on every GA start.
2. **Role suffixes appended at spawn time** — Worker / Orchestrator / cron tasks get extra instructions glued onto the task string.
3. **Bridge rules installed per provider** — `codeterm_rule.md` and `bridge_mem.md` are copied into `~/.claude/rules/` (Claude) or inlined into `AGENTS.md` (Codex) so the provider auto-injects them on every session.
4. **Headless memory workers** — extraction and REM dreaming spawn a hidden agent with a wrapped task. `worker_task_preamble.md` is the wrapper, the inner task body comes from `mem_extraction.md` or `dreaming_rem.md`.
5. **Direct API calls** — `mem_rerank.md` is sent via raw HTTPS to `api.anthropic.com` / OpenAI from `mem/rerank.rs`. The only place in the codebase that talks to a model API without going through the agent provider abstraction.

`.claude/agents/*.md` files are loaded by Claude Code itself when a specialized subagent is spawned (`backend-dev`, `frontend-dev`, `docs`, ...) and bypass everything described above.

## Prompt files

All in-binary prompts live in `src-tauri/prompts/`. Each one is exposed as a `pub const &str` in `src-tauri/src/prompts.rs`.

| File | Const | Category | Placeholders | Injection point |
|---|---|---|---|---|
| `general_agent.md` | `GENERAL_AGENT_TEMPLATE` | GA system prompt | `{version}` `{platform}` `{hostname}` `{provider_name}` `{general_dir}` `{secrets_dir}` `{skills_dir}` `{identity}` `{soul}` | `service/general_agent.rs:114` — substituted, written to `AGENT.md` on every GA start |
| `bootstrap_instructions.md` | `BOOTSTRAP_INSTRUCTIONS_TEMPLATE` | First-run onboarding | — | `service/general_agent.rs:151` — written to `bootstrap.md` once, deleted after GA completes onboarding |
| `worker_suffix.md` | `WORKER_SUFFIX` | Role suffix | — | `service/orchestrator.rs` — appended when spawned with `--role Worker` |
| `orchestrator_suffix.md` | `ORCHESTRATOR_SUFFIX` | Role suffix | — | `service/orchestrator.rs` — appended for `--role Orchestrator` |
| `cron_report_hint.md` | `CRON_REPORT_HINT` | Cron suffix | — | `service/cron.rs` — appended when a cron job has a `creator_pane_id` |
| `cron_complete_suffix.md` | `CRON_COMPLETE_SUFFIX` | Cron suffix | `{}` (run_id) | `service/cron.rs:139`, `cron/scheduler.rs:137` — always appended |
| `codeterm_rule.md` | `CODETERM_RULE` | Framework rule | — | `commands.rs:707` — written to `~/.claude/rules/codeterm.md`; inlined into Codex `AGENTS.md` |
| `bridge_mem.md` | `BRIDGE_MEM` | Memory protocol rule | — | `service/sync_watcher.rs:18` installs per provider; inlined into Codex `AGENTS.md` |
| `worker_task_preamble.md` | `WORKER_TASK_PREAMBLE` | Memory worker wrapper | `{intent}` `{worker_id}` `{task}` | `mem/workers.rs` `build_worker_task()` — wraps every headless memory worker task |
| `mem_extraction.md` | `MEM_EXTRACTION` | Memory worker | `{id}` `{title}` `{body}` | `mem/auto_extract.rs` `build_extraction_prompt()` — entity/relation extraction by haiku |
| `dreaming_rem.md` | `DREAMING_REM` | Memory worker | — | `mem/dreaming.rs` `spawn_rem_worker()` — REM-phase memory consolidation |
| `mem_rerank.md` | `MEM_RERANK` | Direct API call | `{query}` `{items}` | `mem/rerank.rs` `build_rerank_prompt()` — POST to Anthropic/OpenAI for search reranking |

### Placeholder rules

Substitution is plain `String::replace()` — there is no template engine. This means:

- **Placeholder names must stay in sync with Rust.** Rename `{soul}` in `general_agent.md` without touching `service/general_agent.rs`, and the literal text `{soul}` ends up in the model's prompt. There is no compile-time check.
- **Single-pass substitution.** If a placeholder value itself contains another placeholder string (e.g. `{body}` value has `{id}` inside it), the inner one will not be re-expanded — substitutions run in a fixed order (id → title → body in `mem_extraction.md`), and only the template's literal placeholders match.
- **`cron_complete_suffix.md` is the only file using positional `{}`.** Don't add other `{}` sequences to that file.
- **Don't reuse a placeholder name across files** unless the meaning is identical — it makes refactors error-prone.

## Memory worker prompts

Four of the prompts above (`worker_task_preamble`, `mem_extraction`, `dreaming_rem`, `mem_rerank`) are owned by the memory subsystem and used at runtime, but they are still plain `.md` files like everything else — no inline string literals. Quick map:

- **`worker_task_preamble.md`** — wrapper applied by `workers::build_worker_task()` to every headless memory worker. Sets the "you are a Graph Worker" framing, tells the worker how to call back via `codeterm mem worker progress|report`, then injects the inner `{task}`.
- **`mem_extraction.md`** — inner task body for the auto-extraction worker spawned on `mem save` for items longer than ~80 chars (kinds: conversation, note, daily, learning, fact). Worker extracts people, projects, decisions, problems, area; calls back via `codeterm mem graph` commands. The body is truncated to 4000 chars before substitution.
- **`dreaming_rem.md`** — inner task body for the REM phase of the dreaming pipeline. Light, Deep, Normalize, and Calibration phases are pure SQL (no LLM) and run server-side in `mem_dream` before the REM worker spawns; REM is the only LLM-driven phase, fired via `dreaming::spawn_rem_worker()`. Triggered by the bundled `mem-dreaming-auto` cron at 6 AM ±30 min jitter (legacy 3 AM ±90 min installs auto-migrate on startup). The prompt runs four stages — STAGE A (structural cleanup), STAGE B (diary + worker report), STAGE C (maintenance), STAGE D (read-only calibration summary). STAGE D used to instruct the LLM to invoke `mem calibration sweep` itself, which it routinely skipped in resumed sessions and replaced with a fabricated `skipped=not_sunday` line; the sweep is now authoritatively executed by the supervisor and STAGE D only reads `mem calibration log` to format the diary's CALIBRATION line. Server still enforces the Sunday-only gate via `strftime('%w','now')`, the 30-changes-per-session cap, and the 6-hour re-entry guard.
- **`mem_rerank.md`** — sent via raw `reqwest` POST from `mem/rerank.rs` to `api.anthropic.com/v1/messages` (`call_anthropic`) or the OpenAI equivalent. The only place in CodeTerm that talks to a model API directly. Used when search needs higher precision than FTS5 + vector cosine alone. The `{items}` placeholder is built in Rust by formatting each `SearchHit` and joining them.

`mem/seed.rs` ships a handful of baseline `NewMemItem` rows (bundled rules and the codeterm skill stub) inserted on fresh install. These are Rust literals, not LLM prompts — they live in the mem store and are surfaced to agents only via `codeterm mem search`/`get`.

## Role definitions live in `codeterm mem`, not `.claude/agents/`

CodeTerm dogfoods its own mem as the single source of truth for role-specific guidance (see "We dogfood..." section in `CLAUDE.md`). When you spawn a worker, the task prompt instructs it to `codeterm mem get <role_skill_id>` BEFORE doing the concrete work.

Current role skills in mem:

- `skill_backend_dev_role_rust_src_tauri_for_codeterm` — Rust / `src-tauri/` work
- `skill_frontend_dev_role_react_typescript_src_for_codeterm` — React / TypeScript / `src/` work
- `skill_code_reviewer_role_read_only_review_for_codeterm` — read-only diff review
- `skill_docs_sync_role_keep_docs_in_sync_with_code_for_codeterm` — documentation sync
- `skill_prompt_evaluator_role_a_b_and_regression_testing_for_codeterm` — A/B / regression testing for prompt files
- `skill_end_to_end_github_release_for_codeterm` — full release pipeline

Mem entries propagate immediately across all CodeTerm instances on the machine. No rebuild, no per-checkout copies. `.claude/skills/` is kept only for skills with runnable code (e.g. `prompt-evaluator/runner.py`); their `SKILL.md` is the entry point the mem skill points to.

## Loading flow

```
.md file in src-tauri/prompts/
        │
        │  include_str!()  (compile time)
        ▼
src-tauri/src/prompts.rs  (pub const &str)
        │
        ├──► service/general_agent.rs ─► ~/.codeterm/general/AGENT.md     (GA system prompt)
        │                            └─► ~/.codeterm/general/AGENTS.md    (Codex inline bundle)
        │
        ├──► service/orchestrator.rs ─► task_text += SUFFIX                (Worker/Orch spawn)
        │
        ├──► service/cron.rs ──────── ─► task_text += CRON_COMPLETE_SUFFIX (every cron run)
        │
        ├──► commands.rs ──────────── ─► ~/.claude/rules/codeterm.md       (Claude bridge install)
        │
        ├──► service/sync_watcher.rs ─► per-provider bridge rule install   (BRIDGE_MEM)
        │
        ├──► mem/workers.rs ────────── ─► WORKER_TASK_PREAMBLE wraps every headless mem worker
        │       │
        │       ├──► mem/auto_extract.rs ─► spawn_headless (extraction worker)
        │       └──► mem/dreaming.rs ────► spawn_headless (REM worker)
        │
        └──► mem/rerank.rs ──────────── ─► raw HTTPS POST to Anthropic / OpenAI (no agent provider)
```

The General Agent also gets `CODETERM_RULE` and `BRIDGE_MEM` via the provider's rule mechanism, **not** via `AGENT.md`. That is why `AGENTS.md` (for Codex, which has no rules dir) concatenates all three manually — see `service/general_agent.rs:138-143`.

## Editing workflow

1. **Edit the `.md` file** under `src-tauri/prompts/` or the agent definition under `.claude/agents/`.
2. **Rebuild** — `./build.sh dev` picks up changes on next start; nothing reloads live because `include_str!()` runs at compile time. For `.claude/agents/*.md` no rebuild needed.
3. **Verify** — check the running `AGENT.md` actually reflects your change:
   ```bash
   cat ~/.codeterm/general/AGENT.md  # for general_agent.md edits
   cat ~/.claude/rules/codeterm.md   # for codeterm_rule.md edits
   ```
4. **Run the prompt-evaluator** — this is mandatory before committing any prompt change. Spawn it:
   ```
   Agent(subagent_type="prompt-evaluator",
         prompt="A/B compare HEAD~1 vs HEAD for src-tauri/prompts/<file>.md. Report regressions.")
   ```
   If the evaluator reports `Δ < 0` on any eval, fix or revert. Do not push through a regression. If no eval set exists for the file yet, ask the evaluator to bootstrap one first. See `.claude/skills/prompt-evaluator/SKILL.md` and `evals/` for the harness.
5. **Commit** — use `prompts:` or `prompt:` scope in the commit message so prompt changes are easy to find in `git log`.

## Rules

- **Two locations only.** Every prompt belongs in either `src-tauri/prompts/` (in-binary) or `.claude/agents/` (Claude Code subagent definitions). Do not scatter `.md` instructions through `src-tauri/src/`, `cli/`, `src/`, or anywhere else.
- **No inline multi-line prompt strings in Rust or TypeScript.** If you catch yourself writing a `format!("You are an agent that ...")` longer than two lines, stop. Move it to `src-tauri/prompts/<name>.md`, add a const in `prompts.rs`, and use `crate::prompts::<NAME>.replace("{var}", value)`. Even short ones (rerank is ~5 lines) belong in a file — having one source of truth is more important than line count.
- **Every new prompt const goes through `prompts.rs`.** Never `include_str!("../prompts/foo.md")` directly from a sibling module — that fragments the index. The whole point of `prompts.rs` is that you can `grep WORKER_` once and find every prompt in the project.
- **No hot-reload assumptions.** `include_str!()` is compile-time. If a change "isn't taking effect", it is almost always a stale binary — kill and restart dev (`./build.sh kill && ./build.sh dev`); the kill guarantees the old process is gone.
- **Run prompt-evaluator on every prompt change.** This is the only guard against silent behavioral regressions — prompt diffs do not show up in unit tests.
- **Prompts are English-only.** Like all code and docs.

## Related docs

- `docs/GENERAL_AGENT.md` — GA lifecycle and behavior spec (what `general_agent.md` is trying to produce)
- `docs/MEMORY.md` — memory system including the behavioral protocol from `bridge_mem.md`
- `docs/API.md` — "Agent Prompts" section lists the same files from an API-consumer angle
- `.claude/agents/prompt-evaluator.md` — the agent that A/B tests prompt changes
- `.claude/skills/prompt-evaluator/SKILL.md` — the harness it drives

<!-- revision: 7 -->
