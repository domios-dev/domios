# Prompts

How CodeTerm ships instructions to LLM agents: where prompt files live, how they are compiled and injected, and how to edit them safely.

## Overview

Every system-level prompt in CodeTerm is compiled into the binary via `include_str!()`. There is no runtime prompt store and no hot-reload — **editing a prompt requires a rebuild**. Prompt fragments live under `src-tauri/prompts/`; the complete Orchestrator and Planner manuals remain canonical in `docs/ORCHESTRATION.md` and `docs/PLANNER.md` and are embedded into their startup contracts. Loaders are split by ownership: the app catalog is `src-tauri/src/prompts.rs`, plan worker fragments are in `codeterm-plan`, memory prompts and private seed constants are in `codeterm-mem`, and the provider bridge rule is also loaded by `codeterm-plugin`.

**One in-binary prompt location.** Every prompt the project owns and ships with CodeTerm lives here:

1. **`src-tauri/prompts/`** — all in-binary prompt sources. Loaded via `include_str!()` by the owning app or crate module. Includes the GA system prompt, role suffixes, framework rules, bridge memory protocol, cron suffixes, seed prompts, AND every memory-worker prompt (extraction, REM dreaming, reranking, worker preamble).

Provider-specific prompt wrappers and synced skill copies may exist under user home directories at runtime, but they are generated artifacts, not source prompts. Apart from the two canonical role manuals above, a project-owned prompt source outside `src-tauri/prompts/` is a bug.

Prompts reach the model through these pathways:

1. **System prompt of the General Agent** — composed from `general_agent.md`, the bounded L0/L1 wake-up snapshot, `codeterm_rule.md`, and `bridge_mem.md`. Native PTY providers load the generated `CLAUDE.md`/`AGENTS.md` startup channel exactly once; providers declaring `External` receive the identical text through durable confirmed fallback.
2. **System prompt of spawned agents** — `agent_codeterm_primer.md` is prepended once, followed by the assignment and applicable Worker, Planner, or Orchestrator contract. Native providers use their declared system-prompt capability; `External` providers use durable confirmed fallback. ChatBackend sessions receive the same composition through `openSession.systemPrompt`.
3. **Bridge rules installed per provider** — `codeterm_rule.md` and `bridge_mem.md` are copied into `~/.claude/rules/` (Claude) or inlined into `AGENTS.md` (Codex) so the provider auto-injects them on every session.
4. **Headless memory workers** — extraction and REM dreaming spawn a hidden agent with a wrapped task. `worker_task_preamble.md` is the wrapper, the inner task body comes from `mem_extraction.md` or `dreaming_rem.md`.
5. **Plugin-spawned agents** — plugins that spawn agents wrap their task with `plugin_agent_task_preamble.md`, so they act through plugin verbs instead of the memory-worker protocol.
6. **Direct API calls** — `mem_rerank.md` is sent via raw HTTPS to `api.anthropic.com` / OpenAI from `mem/rerank.rs`. The only place in the codebase that talks to a model API without going through the agent provider abstraction.

## Prompt files

All in-binary prompt sources live in `src-tauri/prompts/`. Their loader constants are split across `src-tauri/src/prompts.rs`, `codeterm-plan/src/service_plan/prompts_plan.rs`, `codeterm-mem/src/prompt_text.rs`, `codeterm-mem/src/seed.rs`, and `codeterm-plugin/src/prompts.rs` according to ownership. Seed constants are private; they are not part of a shared prompt catalog.

| File | Const | Category | Placeholders | Injection point |
|---|---|---|---|---|
| `general_agent.md` | `GENERAL_AGENT_TEMPLATE` | GA system prompt | `{version}` `{platform}` `{hostname}` `{provider_name}` `{general_dir}` `{secrets_dir}` `{skills_dir}` `{identity}` `{soul}` `{wake_up}` `{codeterm_cli}` | `service/general_agent/instructions.rs` — substituted, written to `AGENT.md` on every GA start |
| `bootstrap_instructions.md` | `BOOTSTRAP_INSTRUCTIONS_TEMPLATE` | First-run onboarding | — | `service/general_agent/instructions.rs` — written to `bootstrap.md` once, deleted after GA completes onboarding |
| `agent_codeterm_primer.md` | `AGENT_CODETERM_PRIMER` | Spawned-agent system primer, including the task test/QA lifecycle boundary | — | `service/orchestrator/launch.rs` — prepended once before assignment and role contract for PTY and ChatBackend launches |
| `worker_codeterm_context.md` | `WORKER_CODETERM_CONTEXT` | Worker CodeTerm context | — | `service/orchestrator/launch.rs` — injected once into Worker launches before assignment work |
| `rich_rendering_guidance.md` | `RICH_RENDERING_GUIDANCE` | Shared rendering capability | — | `service/orchestrator/launch.rs` — included for capable spawned agents |
| `worker_suffix.md` | `WORKER_SUFFIX` | Role suffix | — | `service/orchestrator.rs` and `service/spawn.rs` — appended when spawned with `--role Worker` |
| `testing_discipline.md` | `TESTING_DISCIPLINE` | Role suffix fragment | — | `service_plan/prompts_plan.rs` — prepended to Worker tasks before `worker_suffix.md` |
| `qa_pipeline_discipline.md` | `QA_PIPELINE_DISCIPLINE` | Role suffix fragment | — | `service_plan/prompts_plan.rs` — prepended to Worker tasks before `worker_suffix.md` |
| `planning_compaction.md` | `PLANNING_COMPACTION_CONTRACT` | Shared planning fragment | — | `prompts.rs` — substituted into both `planner_suffix.md` and `orchestrator_suffix.md` so the role that cuts a task graph and the role that judges it share one decomposition contract |
| `bounce_unwinding_contract.md` | `BOUNCE_UNWINDING_CONTRACT` | Shared protocol + embedded `BOUNCE` doc | — | `cli/src/cmd/docs.rs` exposes the detailed record on demand to both planning roles; deterministic fixtures use the same source |
| `planner_suffix.md` | `PLANNER_SUFFIX` | Compact role prefix | `{test_author_guidance}` `{planning_compaction_contract}` | `prompts.rs` — rendered before the complete embedded `docs/PLANNER.md`; delivered through the provider's system-prompt channel or CodeTerm system-message fallback |
| `orchestrator_suffix.md` | `ORCHESTRATOR_SUFFIX` | Compact role prefix | `{planning_compaction_contract}` | `prompts.rs` — rendered before the complete embedded `docs/ORCHESTRATION.md`; delivered through the provider's system-prompt channel or CodeTerm system-message fallback |
| `orchestrator_onboarding.md` | `ORCHESTRATOR_ONBOARDING` | Promotion onboarding | `{planner_spawn_cmd}` `{orchestrator_suffix}` | `service/orchestrator_onboard.rs` — delivered when an existing agent tab becomes Orchestrator |
| `manager_bootstrap.md` | `MANAGER_BOOTSTRAP_TEMPLATE` | Team manager bootstrap | role/team/plan/project placeholders | `service/role_identity.rs` — shared manager identity rendered before launch delivery |
| `team_bootstrap_starter.md` | `TEAM_BOOTSTRAP_STARTER` | Provider-visible team wake-up turn | — | `codeterm-plugin` host injects one shared starter into every provider launch |
| `cron_report_hint.md` | `CRON_REPORT_HINT` | Cron suffix | — | `service/cron.rs` — appended when a cron job has a `creator_tab_id` |
| `cron_complete_suffix.md` | `CRON_COMPLETE_SUFFIX` | Cron suffix | `{}` (run_id) | `service/cron.rs`, `cron/scheduler.rs` — always appended |
| `cron_skill_hint.md` | `CRON_SKILL_HINT` | Cron skill instruction | `{skill_path}` `{skill_slug}` | `commands/skills.rs` — points a cron agent at the resolved skill |
| `cron_skill_missing.md` | `CRON_SKILL_MISSING` | Cron skill failure instruction | `{skill_slug}` | `commands/skills.rs` — provides a bounded failure handoff when the requested skill is absent |
| `codeterm_rule.md` | `CODETERM_RULE` | Framework rule | — | App loader: `service/general_agent/instructions.rs`; plugin loader: `codeterm-plugin/src/agent_claude/provider/integration.rs` writes `~/.claude/rules/codeterm.md` |
| `codeterm_cli.md` | `CODETERM_CLI` | Framework rule | — | `service/general_agent/instructions.rs` — injected into the GA prompt via `{codeterm_cli}`; also mirrored into the codeterm skill |
| `bridge_mem.md` | `BRIDGE_MEM` | Memory protocol rule | — | `service/sync_watcher.rs:18` installs per provider; inlined into Codex `AGENTS.md` |
| `worker_task_preamble.md` | `WORKER_TASK_PREAMBLE` | Memory worker wrapper | `{intent}` `{worker_id}` `{task}` | `mem/workers.rs` `build_worker_task()` — wraps every headless memory worker task |
| `plugin_agent_task_preamble.md` | `PLUGIN_AGENT_TASK_PREAMBLE` | Plugin agent wrapper | `{task}` | `service/plugin_host.rs` — wraps plugin-spawned agent tasks before launch |
| `mem_extraction.md` | `MEM_EXTRACTION` | Memory worker | `{id}` `{title}` `{body}` | `mem/auto_extract.rs` `build_extraction_prompt()` — entity/relation extraction by haiku |
| `dreaming_rem.md` | `DREAMING_REM` | Memory worker | — | `mem/dreaming.rs` `spawn_rem_worker()` — REM-phase memory consolidation |
| `mem_rerank.md` | `MEM_RERANK` | Direct API call | `{query}` `{items}` | `mem/rerank.rs` `build_rerank_prompt()` — POST to Anthropic/OpenAI for search reranking |
| `seed/identity_placeholder.md` | `IDENTITY_BODY` | Fresh-install seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/rule_never_edit_autogen.md` | `RULE_NEVER_EDIT_AUTOGEN` | Fresh-install seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/rule_save_to_mem.md` | `RULE_SAVE_TO_MEM` | Fresh-install seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/rule_search_before_asking.md` | `RULE_SEARCH_BEFORE_ASKING` | Fresh-install seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/skill_codeterm.md` | `SKILL_CODETERM` | Fresh-install seed | — | private `codeterm-mem/src/seed.rs` constant |

### Placeholder rules

Substitution is plain `String::replace()` — there is no template engine. This means:

- **Placeholder names must stay in sync with Rust.** Rename `{soul}` in `general_agent.md` without touching `service/general_agent.rs`, and the literal text `{soul}` ends up in the model's prompt. There is no compile-time check.
- **Single-pass substitution.** If a placeholder value itself contains another placeholder string (e.g. `{body}` value has `{id}` inside it), the inner one will not be re-expanded — substitutions run in a fixed order (id → title → body in `mem_extraction.md`), and only the template's literal placeholders match.
- **`cron_complete_suffix.md` is the only file using positional `{}`.** Don't add other `{}` sequences to that file.
- **Don't reuse a placeholder name across files** unless the meaning is identical — it makes refactors error-prone.

## Memory worker prompts

Four of the prompts above (`worker_task_preamble`, `mem_extraction`, `dreaming_rem`, `mem_rerank`) are owned by the memory subsystem and used at runtime, but they are still plain `.md` files like everything else — no inline string literals. Quick map:

- **`worker_task_preamble.md`** — wrapper applied by `workers::build_worker_task()` to every headless memory worker. Sets the "you are a Graph Worker" framing, tells the worker how to call back via `codeterm mem worker progress|report`, then injects the inner `{task}`.
- **`mem_extraction.md`** — inner task body for the auto-extraction worker spawned on `mem save` unless `skip_extract` is set. The code excludes only `MemKind::Summary`; all other kinds are eligible. Worker extracts people, projects, decisions, problems, area; calls back via `codeterm mem graph` commands. The body is truncated to 50,000 chars before substitution.
- **`dreaming_rem.md`** — inner task body for the REM phase of the dreaming pipeline. Light, Deep, Normalize, and Calibration phases are pure SQL (no LLM) and run server-side in `mem_dream` before the REM worker spawns; REM is the only LLM-driven phase, fired via `dreaming::spawn_rem_worker()`. Triggered by the bundled `mem-dreaming-auto` cron at 6 AM with a 0..30 min delay-only jitter window (legacy 3 AM with 0..90 min delay installs auto-migrate on startup). The prompt runs four stages — STAGE A (structural cleanup), STAGE B (diary + worker report), STAGE C (maintenance), STAGE D (read-only calibration summary). STAGE D used to instruct the LLM to invoke `mem calibration sweep` itself, which it routinely skipped in resumed sessions and replaced with a fabricated `skipped=not_sunday` line; the sweep is now authoritatively executed by the supervisor and STAGE D only reads `mem calibration log` to format the diary's CALIBRATION line. Server still enforces the Sunday-only gate via `strftime('%w','now')`, the 30-changes-per-session cap, and the 6-hour re-entry guard.
- **`mem_rerank.md`** — sent via raw `reqwest` POST from `mem/rerank.rs` to `api.anthropic.com/v1/messages` (`call_anthropic`) or the OpenAI equivalent. The only place in CodeTerm that talks to a model API directly. Used when search needs higher precision than FTS5 + vector cosine alone. The `{items}` placeholder is built in Rust by formatting each `SearchHit` and joining them.

## Plugin-spawned agent prompts

`plugin_agent_task_preamble.md` is the wrapper applied by `service/plugin_host.rs` when a plugin spawns an agent through the host API. It injects the plugin-owned `{task}` and keeps that agent out of the memory graph-worker protocol: plugin agents should operate through verbs exposed as `codeterm plugin <id> <verb>`, which dispatches to the plugin's `onAgentCommand` handler.

`codeterm-mem/src/seed.rs` privately loads markdown sources under `prompts/seed/` and inserts them as baseline `NewMemItem` rows on fresh install. They live in the mem store and are surfaced to agents only via `codeterm mem search`/`get`.

## Role presets — one resolver, one composition table

There is exactly one persona registry. `service/role_preset.rs` resolves a
`(role, role_profile)` pair to a `RolePreset { role, kind_id, label, fragments }`,
keyed by kind id and never by a role or provider literal. Every Manager kind and
every Worker subtype goes through it; there is no separate Worker path.

A preset's guidance is a list of fragments, each with its own provenance, joined
with a blank line in this order:

| Fragment | Source | Meaning |
|---|---|---|
| `builtin` | `role_preset::builtin_guidance` | Bundled text for a builtin kind |
| `inherited_base` | `worker_suffix.md` | The base a leaf kind inherits when it ships no bundled text |
| `disk_overlay` | `<data-dir>/role-presets/*.toml` | Runtime-declared leaf guidance |
| `task_overlay` | The dispatch | Task personality, `role_guidance`, and `executor_role` for this one delivery |

`service/persona/occasion.rs` holds the single declarative table mapping a
delivery occasion to the fragments it carries, the separator it uses, and whether
it speaks with no payload. Occasions:

| Occasion | Injection point | Fragments | Declared fallback |
|---|---|---|---|
| `launch_assignment` | `service/orchestrator/launch.rs` | contract (`builtin`/`inherited_base` + `disk_overlay`) | the assignment alone |
| `worker_launch_identity` | `service/orchestrator/launch.rs` | `task_overlay` only | `You are a CodeTerm Worker.` |
| `manager_bootstrap` | `service/role_identity.rs` | contract | `planner_suffix.md` |

Every occasion declares a fallback, and an unmatched `(occasion, kind)` pair
lands on it rather than on silence. Worker task personality is not a private
path: it enters as the `task_overlay` fragment of the same `RolePreset` a
manager kind resolves, and only the `worker_launch_identity` occasion places it.

### Runtime overlays

Files in `<data-dir>/role-presets/*.toml` add leaf kinds without a rebuild
(`codeterm persona reload` re-reads them):

```toml
id = "rust-backend-dev"
label = "Rust Backend Dev"
base = "worker"                        # optional; only a leaf base is admissible
guidance = "Stay inside src-tauri/."
```

An overlay may add a kind id, a label, and layered guidance. It may never do
anything else, and each refusal has its own reason, reported by
`codeterm persona list`:

| Reason | Trigger |
|---|---|
| `builtin_override` | `id` names a builtin kind |
| `capability_escalation` | declares `anchors_team`, `can_spawn`, `team_authority`, `requires_parent`, `capability`, `origin`, or claims an anchoring base |
| `team_authority_claim` | claims the `planner` or `watcher` base |
| `missing_core` | the contract it claims would ship without the Worker framework rule or the report protocol |
| `malformed` / `unreadable` / `empty_guidance` | the file cannot be used at all |

A refused file is skipped and logged; it never blocks a healthy sibling, and an
overlay kind always resolves to leaf capability — it can never anchor a Team,
spawn, or hold Team authority.

### Inspecting the resolver

`codeterm persona list` shows every reachable kind with its capability columns
and the refused overlays. `codeterm persona show --role Worker --kind
rust-backend-dev --guidance "You are Mira."` renders the exact text each
occasion emits, which fragment supplied it, and whether the declared fallback
was used. The text comes from the shared resolver, so what it prints is what the
agent receives.

Reusable operational knowledge belongs in a named embedded document or
folder skills when runnable files are required. Machine-local preferences such as
builder capacity belong in mem preferences.

## Task verification lifecycle

Task `test_plan` is the implementer's focused evidence and is included in the Worker
brief. Task `qa_plan` is the independent Orchestrator/QA oracle, is not included in that
brief, and falls back to `test_plan` only when omitted. This keeps implementation and
acceptance evidence distinct without requiring every task to repeat the same command.

## Loading flow

```
.md file in src-tauri/prompts/
        │
        │  include_str!()  (compile time)
        ▼
owning loader module (pub const &str)
        │
        ├──► app prompts.rs ─► service/orchestrator/launch.rs (rendered role suffixes)
        │                    └─► service/plugin_host.rs (plugin-agent wrapper)
        ├──► codeterm-plan/prompts_plan.rs ─► worker task rendering
        ├──► codeterm-mem/prompt_text.rs ─► extraction, dreaming, and reranking
        ├──► codeterm-mem/seed.rs (private consts) ─► fresh-install memory seeds
        └──► codeterm-plugin/prompts.rs ─► agent_claude/provider/integration.rs
```

`codeterm_rule.md` deliberately has two `include_str!` loaders: the app copy feeds `service/general_agent/instructions.rs`, while the `codeterm-plugin` copy feeds provider bridge installation. The active role and plugin-agent injection paths are `service/orchestrator/launch.rs` and `service/plugin_host.rs`; the loader modules only own compilation and rendering.

## Editing workflow

1. **Edit the `.md` file** under `src-tauri/prompts/`.
2. **Rebuild** — `./build.sh dev` picks up changes on next start; nothing reloads live because `include_str!()` runs at compile time.
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

- **One source location.** Every project-owned prompt belongs in `src-tauri/prompts/`. Do not scatter `.md` instructions through `src-tauri/src/`, `cli/`, `src/`, or anywhere else.
- **No inline multi-line prompt strings in Rust or TypeScript.** If you catch yourself writing a `format!("You are an agent that ...")` longer than two lines, stop. Move it to `src-tauri/prompts/<name>.md`, add a const in the owning loader module, and render it there. Even short ones (rerank is ~5 lines) belong in a file — having one source of truth is more important than line count.
- **Every new prompt const has one authoritative owning loader.** App-wide prompts go through `src-tauri/src/prompts.rs`; crate-owned prompts stay in that crate's prompt module. Update `scripts/check-prompts-doc.sh` when a new owning loader is introduced so the source and documentation inventories remain exact.
- **No hot-reload assumptions.** `include_str!()` is compile-time. If a change "isn't taking effect", it is almost always a stale binary — kill and restart dev (`./build.sh kill && ./build.sh dev`); the kill guarantees the old process is gone.
- **Run prompt-evaluator on every prompt change.** This is the only guard against silent behavioral regressions — prompt diffs do not show up in unit tests.
- **No Markdown bold in compiled prompts.** Use plain prose or uppercase for essential labels; `**` spends context tokens without adding agent capability.
- **Prompts are English-only.** Like all code and docs.

## Related docs

- `docs/GENERAL_AGENT.md` — GA lifecycle and behavior spec (what `general_agent.md` is trying to produce)
- `docs/MEMORY.md` — memory system including the behavioral protocol from `bridge_mem.md`
- `docs/API.md` — "Agent Prompts" section lists the same files from an API-consumer angle
- `.claude/skills/prompt-evaluator/SKILL.md` — the harness it drives


## Prompt tiers — progressive disclosure

System-prompt content is tiered; keep it that way when editing:

- **Tier 0 (always in context):** hard invariants not discoverable via help
  (routing, no-echo, chat_backend-no-shell, CLI-first discovery habit) plus a
  capability map — one line per domain pointing at its tier-1 entry.
  `codeterm_cli.md` budget: ~900 words. Mem and mesh keep compact protocol
  blocks by explicit decision (high-frequency, behavior-critical).
- **Tier 1 (agent discovers):** `codeterm <cmd> --help` (clap long_help) and
  the matching complete document. Nothing may live inline that tier 1 already answers;
  when adding a rule, put the rationale in docs and one imperative line here.
- **Tier 2:** situational playbooks (TELEGRAM-SETUP, SECRETS, ...).

Behavioral prompt changes are gated by `evals/cli-agent` (baseline vs after)
with sandboxes OUTSIDE the repo — in-repo sandboxes feed agents the repo's
checked-in AGENTS.md instead of the generated one and invalidate the run.

<!-- revision: 24 -->
