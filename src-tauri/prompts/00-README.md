# Prompt catalog
How Domios ships instructions to LLM agents: where prompt files live, how they are compiled and injected, and how to edit them safely.

This README is source documentation, not an injected prompt. The table below identifies each prompt, its audience, placeholders and injection point.

## Overview
Every system-level prompt in Domios is compiled into the binary via `include_str!()`. There is no runtime prompt store and no hot-reload — **editing a prompt requires a rebuild**. Host prompt fragments live under `src-tauri/prompts/`; builtin role prose lives once in the default team package under `src-tauri/team-templates/default/` (see *Role files*), whose Orchestrator and Planner files also serve `domios docs ORCHESTRATION` and `domios docs PLANNER`. Loaders are split by ownership: the app catalog is in `src-tauri/src/prompts.rs`, shared runtime fragments in `codeterm-service-core`, plan worker fragments in `codeterm-plan`, memory prompts and private seed constants in `codeterm-mem`, and the provider bridge rule is also loaded by `codeterm-plugin`.

**Two in-binary prompt roots.** Host prompts live here; builtin role prose lives in the default team package:

1. **`src-tauri/prompts/`** — all in-binary prompt sources. Loaded via `include_str!()` by the owning app or crate module. Includes the GA system prompt, host runtime text appended to roles, bridge memory protocol, cron suffixes, seed prompts, AND every memory-worker prompt (extraction, REM dreaming, reranking, worker preamble).
2. **`src-tauri/team-templates/default/`** — the builtin default package: `team.yaml` plus one role file per manager slot and worker. Embedded by `codeterm-team-package` and read by `codeterm-prompts/src/builtin_roles.rs`.

Provider-specific prompt wrappers and synced skill copies may exist under user home directories at runtime, but they are generated artifacts, not source prompts. A project-owned prompt source outside these two roots is a bug.

Prompts reach the model through these pathways:

1. **System prompt of the General Agent** — composed from `general_agent.md`, the bounded L0/L1 wake-up snapshot, `codeterm_rule.md`, and `bridge_mem.md`. Native PTY providers load the generated `CLAUDE.md`/`AGENTS.md` startup channel exactly once; providers declaring `External` receive the identical text through durable confirmed fallback.
2. **System prompt of spawned agents** — `agent_codeterm_primer.md` and `agent_routing_contract.md` lead the shared launch fragments, followed by the assignment and applicable Worker, Planner, or Orchestrator contract. Native providers use their declared system-prompt capability; `External` providers use durable confirmed fallback. ChatBackend sessions receive the same composition through `openSession.systemPrompt`.
3. **Bridge rules installed per provider** — `codeterm_rule.md` and `bridge_mem.md` are copied into `~/.claude/rules/` (Claude) or inlined into `AGENTS.md` (Codex) so the provider auto-injects them on every session.
4. **Headless memory workers** — extraction and REM dreaming spawn a hidden agent with a wrapped task. `worker_task_preamble.md` is the wrapper, the inner task body comes from `mem_extraction.md` or `dreaming_rem.md`.
5. **Plugin-spawned agents** — plugins that spawn agents wrap their task with `plugin_agent_task_preamble.md`, so they act through plugin verbs instead of the memory-worker protocol.
6. **Direct API calls** — `mem_rerank.md` is sent via raw HTTPS to `api.anthropic.com` / OpenAI from `mem/rerank.rs`. The only place in the codebase that talks to a model API without going through the agent provider abstraction.

## Prompt files
All in-binary prompt sources live in `src-tauri/prompts/`. Their loader constants are split across `src-tauri/src/prompts.rs`, `codeterm-service-core/src/prompts.rs`, `codeterm-plan/src/service_plan/prompts_plan.rs`, `codeterm-mem/src/prompt_text.rs`, `codeterm-mem/src/seed.rs`, and `codeterm-plugin/src/prompts.rs` according to ownership. Seed constants are private; they are not part of a shared prompt catalog.

| File | Const | Category | Placeholders | Injection point |
|---|---|---|---|---|
| `mcp_tools.json` | `DESCRIPTIONS` | MCP tool and argument descriptions | — | `cli/src/cmd/mcp/tools.rs` — `tools/list` |
| `general_agent.md` | `GENERAL_AGENT_TEMPLATE` | GA system prompt | `{version}` `{platform}` `{hostname}` `{provider_name}` `{general_dir}` `{secrets_dir}` `{skills_dir}` `{identity}` `{soul}` `{wake_up}` `{codeterm_cli}` `{interactive_child_pty_operation}` | `service/general_agent/instructions.rs` — substituted, written to `AGENT.md` on every GA start |
| `interactive_child_pty.md` | `INTERACTIVE_CHILD_PTY_OPERATION` | Shared child PTY recovery procedure | — | `service/general_agent/instructions.rs`, `prompts.rs`, and `service/skill_mirror.rs` — delivered to the General Agent and codeterm skill; the Orchestrator trigger refers to it by capability id |
| `bootstrap_instructions.md` | `BOOTSTRAP_INSTRUCTIONS_TEMPLATE` | First-run onboarding | — | `service/general_agent/instructions.rs` — written to `bootstrap.md` once, deleted after GA completes onboarding |
| `plan_task_flags.md` | `PLAN_TASK_FLAGS` | Manager host text | — | `role_preset.rs` — appended after the Orchestrator, Planner and Watcher prose |
| `plugin_discovery_rule.md` | `PLUGIN_DISCOVERY_RULE` | Capability discovery pointer | — | `prompts.rs` — composed into agent control guidance |
| `agent_codeterm_primer.md` | `AGENT_CODETERM_PRIMER` | Spawned-agent system primer, including the task test/QA lifecycle boundary | — | `service/orchestrator/prompt_compose.rs` — prepended once before assignment and role contract for PTY and ChatBackend launches |
| `agent_routing_contract.md` | `AGENT_ROUTING_CONTRACT` | Shared routed-message trust and reply policy | — | `codeterm-service-core/src/prompts.rs`; composed after the Agent primer for spawned launches in `service/orchestrator/prompt_compose.rs` and `service/orchestrator/spawn/runtime_prompt.rs` |
| `utility_tab_guidance.md` | `UTILITY_TAB_GUIDANCE` | Three-line utility lifecycle and repeated-monitoring guidance | — | `service/orchestrator/prompt_compose.rs` — non-Worker launches only |
| `worker_codeterm_context.md` | `WORKER_CODETERM_CONTEXT` | Worker Domios context | — | `service/orchestrator/prompt_compose.rs` — injected once into Worker launches before assignment work |
| `read_only_worker_guidance.md` | `READ_ONLY_WORKER_GUIDANCE` | Read-only intent authority, explicit-sandbox refusals and stdin reports | — | Worker launch composition replaces the writable-pad fragment for `read_only` execution intent |
| `rich_rendering_guidance.md` | `RICH_RENDERING_GUIDANCE` | Shared rendering capability | — | `service/orchestrator/prompt_compose.rs` — included for capable spawned agents |
| `agent_pad_rule.md` | `AGENT_PAD_RULE` | Shared AgentPad contract — the one semantic source | — | `service/orchestrator/prompt_compose.rs` for every spawned class; `service/general_agent/instructions.rs` (`general_cli_reference`) for the GA; `service/skill_mirror.rs` renders it into the `codeterm` skill via `{agent_pad_rule}` |
| `tool_less.md` | `TOOL_LESS` | Tool-less provider class rules | - | `prompts.rs` `TOOL_LESS` - materialized by `service/tool_less_instructions.rs` for providers declaring the tool-less capability (external delivery as a read-only file, native delivery composed with the assignment) |
| `testing_discipline.md` | `TESTING_DISCIPLINE` | Worker host text | — | `role_preset.rs` — appended after every Worker type's prose; also the worker charter |
| `qa_pipeline_discipline.md` | `QA_PIPELINE_DISCIPLINE` | Worker host text | — | `role_preset.rs` — appended after every Worker type's prose; also the worker charter and Orchestrator heartbeat |
| `orchestrator_onboarding.md` | `ORCHESTRATOR_ONBOARDING` | Promotion onboarding | `{infra_brief}` | `service/orchestrator_onboard.rs` — delivered when an existing agent tab becomes Orchestrator |
| `manager_bootstrap.md` | `MANAGER_BOOTSTRAP_TEMPLATE` | Team manager bootstrap | role/team/plan/project placeholders | `service/role_identity.rs` — shared manager identity rendered before launch delivery |
| `team_bootstrap_starter.md` | `TEAM_BOOTSTRAP_STARTER` | Provider-visible team wake-up turn | — | `codeterm-plugin` host injects one shared starter into every provider launch |
| `cron_report_hint.md` | `CRON_REPORT_HINT` | Cron suffix | — | `cron/task_text.rs` — appended when a cron job has a `report_target_tab_id` |
| `cron_complete_suffix.md` | `CRON_COMPLETE_SUFFIX` | Cron suffix | `{}` (run_id) | `service/cron.rs`, `cron/scheduler.rs` — always appended |
| `cron_skill_hint.md` | `CRON_SKILL_HINT` | Cron skill instruction | `{skill_path}` `{skill_slug}` | `cron/skill_block.rs` — points a cron agent at the resolved skill |
| `cron_skill_missing.md` | `CRON_SKILL_MISSING` | Cron skill failure instruction | `{skill_slug}` | `cron/skill_block.rs` — provides a bounded failure handoff when the requested skill is absent |
| `codeterm_rule.md` | `CODETERM_RULE` | Framework rule | — | App loader: `service/general_agent/instructions.rs`; plugin loader: `codeterm-plugin/src/agent_claude/provider/integration.rs` writes `~/.claude/rules/domios.md` |
| `codeterm_cli.md` | `CODETERM_CLI` | Framework rule | — | `service/general_agent/instructions.rs` — injected into the GA prompt via `{codeterm_cli}`; `service/skill_mirror.rs` renders the same constant into the `codeterm` skill through the same placeholder |
| `mem_management.md` | `MEM_MANAGEMENT` | Shared memory management | — | GA renderer, roleless agent onboarding, and host text after the Orchestrator, Planner and Watcher prose; excluded from Workers, Cartographer and Curator |
| `bridge_mem.md` | `BRIDGE_MEM` | Memory protocol rule | — | `service/sync_watcher.rs:18` installs per provider; inlined into Codex `AGENTS.md` |
| `worker_task_preamble.md` | `WORKER_TASK_PREAMBLE` | Memory worker wrapper | `{intent}` `{worker_id}` `{task}` | `mem/workers.rs` `build_worker_task()` — wraps every headless memory worker task |
| `plugin_agent_task_preamble.md` | `PLUGIN_AGENT_TASK_PREAMBLE` | Plugin agent wrapper | `{task}` | `service/plugin_host.rs` — wraps plugin-spawned agent tasks before launch |
| `mem_extraction.md` | `MEM_EXTRACTION` | Memory worker | `{id}` `{title}` `{body}` | `mem/auto_extract.rs` `build_extraction_prompt()` — entity/relation extraction by haiku |
| `dreaming_rem.md` | `DREAMING_REM` | Memory worker | — | `mem/dreaming.rs` `spawn_rem_worker()` — REM-phase memory consolidation |
| `mem_rerank.md` | `MEM_RERANK` | Direct API call | `{query}` `{items}` | `mem/rerank.rs` `build_rerank_prompt()` — POST to Anthropic/OpenAI for search reranking |
| `blueprint_enrichment.md` | `BLUEPRINT_ENRICHMENT` | Memory worker | `{document}` `{gists}` | `mem/blueprint_library/enrichment.rs` `enrichment_task()` — hidden worker writes a retrieval-only summary and extra intents for a kept Blueprint |
| `seed/identity_placeholder.md` | `IDENTITY_BODY` | Bundled seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/rule_never_edit_autogen.md` | `RULE_NEVER_EDIT_AUTOGEN` | Bundled seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/rule_save_to_mem.md` | `RULE_SAVE_TO_MEM` | Bundled seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/rule_search_before_asking.md` | `RULE_SEARCH_BEFORE_ASKING` | Bundled seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/skill_codeterm.md` | `SKILL_CODETERM` | Bundled seed | — | private `codeterm-mem/src/seed.rs` constant |
| `seed/legacy_1_10_7/identity_placeholder.md` | `LEGACY_1_10_7_IDENTITY_BODY` | Frozen 1.10.7 seed | — | never-edit bootstrap |
| `seed/legacy_1_10_7/rule_never_edit_autogen.md` | `LEGACY_1_10_7_RULE_NEVER_EDIT_AUTOGEN` | Frozen 1.10.7 seed | — | never-edit bootstrap |
| `seed/legacy_1_10_7/rule_save_to_mem.md` | `LEGACY_1_10_7_RULE_SAVE_TO_MEM` | Frozen 1.10.7 seed | — | never-edit bootstrap |
| `seed/legacy_1_10_7/rule_search_before_asking.md` | `LEGACY_1_10_7_RULE_SEARCH_BEFORE_ASKING` | Frozen 1.10.7 seed | — | never-edit bootstrap |
| `seed/legacy_1_10_7/skill_codeterm.md` | `LEGACY_1_10_7_SKILL_CODETERM` | Frozen 1.10.7 seed | — | never-edit bootstrap |

### Placeholder rules
Substitution is plain, ordered `String::replace()` with no template engine:

- Keep placeholder names synchronized with Rust; unresolved names ship literally.
- Values are single-pass and never re-expanded; `mem_extraction.md` orders id → title → body.
- Only `cron_complete_suffix.md` uses positional `{}`.
- Reuse a placeholder name only when its meaning is identical.

### One fragment, every class: the AgentPad rule
`agent_pad_rule.md` is the single semantic source for the pad contract, with three carriers and no copies:

- **Spawned agents** — `service/orchestrator/prompt_compose.rs` composes it into every launch prompt: solo, Orchestrator, Planner, watcher, and Worker, with or without an assignment.
- **General Agent** — `general_cli_reference()` appends it to the CLI reference unconditionally, so a moved section heading cannot drop it.
- **`domios` skill** — `service/skill_mirror.rs` substitutes it into `skills/domios/SKILL.md` through `{agent_pad_rule}` at seed time, including launches delivered as a bare assignment rather than a composed system prompt.

Delivery tests assert constant identity, never wording: `prompt_compose::tests` covers each launch class, `skills_seed_tests::seed_renders_shared_codeterm_fragments_from_constants` covers the skill, the GA writer covers `AGENT.md`/`AGENTS.md`, and `prompts.rs::agent_pad_rule_has_exactly_one_source` rejects duplicate sources.

### CLI grammar in prompts

A fragment names a command once and points at its owning document for the rest.
`prompt_cli_references_exist_in_manifest` resolves every surviving command reference
against `generated/cli/cli-manifest.json`, so a rename cannot leave dead guidance. It validates
references, never prompt wording.

## Memory worker prompts
Five of the prompts above (`worker_task_preamble`, `mem_extraction`, `dreaming_rem`, `mem_rerank`, `blueprint_enrichment`) are owned by the memory subsystem and used at runtime, but they are still plain `.md` files like everything else — no inline string literals. Quick map:

- **`worker_task_preamble.md`** — wrapper applied by `workers::build_worker_task()` to every headless memory worker. Sets the "you are a Graph Worker" framing, tells the worker how to call back via `domios mem worker progress|report`, then injects the inner `{task}`.
- **`mem_extraction.md`** — inner task body for the auto-extraction worker spawned on `mem save` unless `skip_extract` is set. The code excludes only `MemKind::Summary`; all other kinds are eligible. Worker extracts people, projects, decisions, problems, area; calls back via `domios mem graph` commands. The body is truncated to 50,000 chars before substitution.
- **`dreaming_rem.md`** — inner task body for REM, the only LLM-driven dreaming phase; Light, Deep, Normalize, and Calibration run as server-side SQL in `mem_dream` before `dreaming::spawn_rem_worker()` fires it. The bundled `mem-dreaming-auto` cron triggers it daily (see RUNTIME-CONFIG). A capped stage checklist opens it; it uses CLI JSON only, never scripts: a hidden run cannot answer approvals. Stages: A structural cleanup (A.1 from hub `clusters`; A.3 merges duplicates with the reversible `mem merge`, never `mem delete`), B diary + worker report listing merged and archived ids with reasons, C maintenance, D read-only calibration summary. The supervisor runs the calibration sweep and gated stale archive; STAGE D only reads `mem calibration log`. Server-side gates and caps: `domios docs API-MEMORY`.
- **`blueprint_enrichment.md`** — inner task body for the Blueprint enrichment worker. It sees only the stored, value-abstracted YAML and the step gists, and reports one `WHAT: … || INTENT: …` line through `domios mem worker report`. The result is untrusted and used for retrieval only, keyed by the document hash.
- **`mem_rerank.md`** — sent via raw `reqwest` POST from `mem/rerank.rs` to `api.anthropic.com/v1/messages` (`call_anthropic`) or the OpenAI equivalent. The only place in Domios that talks to a model API directly. Used when search needs higher precision than FTS5 + vector cosine alone. The `{items}` placeholder is built in Rust by formatting each `SearchHit` and joining them.

## Plugin-spawned agent prompts

`plugin_agent_task_preamble.md` is the wrapper applied by `service/plugin_host.rs` when a plugin spawns an agent through the host API. It injects the plugin-owned `{task}` and keeps that agent out of the memory graph-worker protocol: plugin agents should operate through verbs exposed as `domios plugin <id> <verb>`, which dispatches to the plugin's `onAgentCommand` handler.

`codeterm-mem/src/seed.rs` privately loads markdown sources under `prompts/seed/` as baseline memory items on every startup. Overwrite authority is each item's `seed_fingerprint` (see `prod-docs/MEMORY.md`): an edited seed reaches a machine on restart only while nothing else has changed that item; a real edit through `mem update` or the API clears the fingerprint, and the item is left alone from then on. Seeds reach agents only through `domios mem search`/`get`.

## Role files

Builtin role prose lives once in the default package. `check-prompts-doc.sh` requires a row here for every role file; `codeterm-team-package` requires `team.yaml` to reference each one. Role prose carries no `{placeholder}`: host runtime text is appended after it per group and manager slot, and a package can never remove or replace that text.

| Role file | Group | Host text appended after it |
|---|---|---|
| `default/managers/orchestrator.md` | Manager slot `orchestrator`; served as `domios docs ORCHESTRATION` | `plan_task_flags.md`, `mem_management.md` |
| `default/managers/planner.md` | Manager slot `planner` and the host-only `watcher`; served as `domios docs PLANNER` | `plan_task_flags.md`, `mem_management.md` |
| `default/managers/cartographer.md` | Manager slot `cartographer` | — |
| `default/managers/curator.md` | Manager slot `curator`, including its ask rubric | — |
| `default/workers/worker.md` | Worker type `worker`; the base of an unregistered Worker type | `testing_discipline.md`, `qa_pipeline_discipline.md` |
| `default/workers/qa.md` | Worker type `qa` | `testing_discipline.md`, `qa_pipeline_discipline.md` |
| `default/workers/reviewer.md` | Worker type `reviewer` | `testing_discipline.md`, `qa_pipeline_discipline.md` |
| `default/workers/release_manager.md` | Worker type `release_manager` | `testing_discipline.md`, `qa_pipeline_discipline.md` |

Every role then receives the Team's worker types with their descriptions, plus its own registered files and skills.

## Role presets — one resolver, one composition table

`service/role_preset.rs` is the sole role registry. It resolves a role type by
precedence task > Team registration > builtin default package, by kind
capability, never by provider literal or a separate Worker path. An omitted
manager slot falls back to the default package; a same-named Team worker
replaces the builtin entirely. Registrations live per Team in
`<data-dir>/teams/<team_id>/roles.json` (`service/team_roles.rs`), are immutable
once stored, and never change a role type's structural capability.

A preset's guidance is a list of fragments, each with its own provenance, joined
with a blank line in this order:

| Fragment | Source | Meaning |
|---|---|---|
| `builtin` | the default package role file | Prose for a builtin role type |
| `team_registration` | the Team's `roles.json` | Prose pinned to the registering package |
| `inherited_base` | `default/workers/worker.md` | The base an unregistered Worker type reads |
| `host` | `src-tauri/prompts/` | Host runtime text per group and manager slot, then the Team's worker types |
| `task_overlay` | The dispatch | Task personality, `role_guidance`, and `executor_role` for this one delivery |

`service/persona/occasion.rs` holds the single declarative table mapping a
delivery occasion to the fragments it carries, the separator it uses, and whether
it speaks with no payload. Occasions:

| Occasion | Injection point | Fragments | Declared fallback |
|---|---|---|---|
| `launch_assignment` | `service/orchestrator/prompt_compose.rs` | contract (role prose + `host`) | the assignment alone |
| `worker_launch_identity` | `service/persona/occasion.rs` | `task_overlay` only | `You are a Domios Worker.` |
| `manager_bootstrap` | `service/role_identity.rs` | contract | the builtin Planner contract |

Every occasion declares a fallback, and an unmatched `(occasion, kind)` pair
lands on it rather than on silence. Worker task personality is not a private
path: it enters as the `task_overlay` fragment of the same `RolePreset` a
manager kind resolves, and only the `worker_launch_identity` occasion places it.

### Inspecting the resolver

`domios role list --team <team-id>` shows every role type the Team can launch
with its package provenance and capability columns. `domios role show checker
--team <team-id> --guidance "You are Mira."` renders the exact text each
occasion emits, which fragment supplied it, and whether the declared fallback
was used. `domios role reload --team <team-id>` re-reads the Team's
registrations from disk.

Reusable operations belong in embedded docs or folder skills; machine-local preferences belong in mem.

## Task verification lifecycle

Task `test_plan` is the Worker's focused evidence. `qa_plan` is the independent
Orchestrator oracle and falls back to `test_plan` only when omitted.

## Product and repository boundary

Embedded prompts describe Domios capabilities and general agent behavior. This repository's build commands, source paths, machine setup and documentation budgets belong in project instructions or skills. Tool-specific runtime contracts remain valid; do not replace them with assumptions about another project. Frozen `seed/legacy_1_10_7/` payloads are recognition fixtures, not current guidance.

## Loading flow

```
.md file in src-tauri/prompts/
        │
        │  include_str!()  (compile time)
        ▼
owning loader module (pub const &str)
        │
        ├──► app prompts.rs ─► service/orchestrator/prompt_compose.rs (role composition)
        │                    └─► service/plugin_host.rs (plugin-agent wrapper)
        ├──► codeterm-plan/prompts_plan.rs ─► worker task rendering
        ├──► codeterm-mem/prompt_text.rs ─► extraction, dreaming, and reranking
        ├──► codeterm-mem/seed.rs (private consts) ─► fresh-install memory seeds
        └──► codeterm-plugin/prompts.rs ─► agent_claude/provider/integration.rs
```

`codeterm_rule.md` deliberately has two `include_str!` loaders: the app copy feeds `service/general_agent/instructions.rs`, while the `codeterm-plugin` copy feeds provider bridge installation. The active role and plugin-agent injection paths are `service/orchestrator/prompt_compose.rs` and `service/plugin_host.rs`; the loader modules only own compilation and rendering.

## Editing workflow

1. **Edit the `.md` file** under `src-tauri/prompts/`.
2. **Rebuild** — `./build.sh dev` picks up changes on next start; nothing reloads live because `include_str!()` runs at compile time.
3. **Verify** — check the running `AGENT.md` actually reflects your change:
   ```bash
   cat ~/.domios/general/AGENT.md  # for general_agent.md edits
   cat ~/.claude/rules/domios.md   # for codeterm_rule.md edits
   ```
4. **Run the prompt-evaluator** before committing any prompt change: A/B compare `HEAD~1` against `HEAD` for the edited file. If any eval reports `Δ < 0`, fix or revert; if the file has no eval set, have the evaluator bootstrap one first. Harness: `.claude/skills/prompt-evaluator/SKILL.md` and `evals/`.
5. **Commit** — use `prompts:` or `prompt:` scope in the commit message so prompt changes are easy to find in `git log`.

## Rules

- **One source location.** Every project-owned prompt belongs in `src-tauri/prompts/`. Do not scatter `.md` instructions through `src-tauri/src/`, `cli/`, `src/`, or anywhere else.
- **No inline multi-line prompt strings in Rust or TypeScript.** If you catch yourself writing a `format!("You are an agent that ...")` longer than two lines, stop. Move it to `src-tauri/prompts/<name>.md`, add a const in the owning loader module, and render it there. Even short ones (rerank is ~5 lines) belong in a file — having one source of truth is more important than line count.
- **Every new prompt const has one authoritative owning loader.** App-wide prompts go through `src-tauri/src/prompts.rs`; crate-owned prompts stay in that crate's prompt module. Update `scripts/check-prompts-doc.sh` when a new owning loader is introduced so the source and documentation inventories remain exact.
- **No hot-reload assumptions.** `include_str!()` is compile-time. If a change "isn't taking effect", it is almost always a stale binary — kill and restart dev (`./build.sh kill && ./build.sh dev`); the kill guarantees the old process is gone.
- **Run prompt-evaluator on every prompt change.** This is the only guard against silent behavioral regressions — prompt diffs do not show up in unit tests.
- **No Markdown bold in compiled prompts.** Use plain prose or uppercase for essential labels; `**` spends context tokens without adding agent capability.
- **Prompts are English-only.** Like all code and docs.
- **State a rule once, at its owning fragment, in a calm voice.** A role brief carries only what its embedded manual does not already say; capitals and absolutes are for the few hard invariants. Composed role launches, the Worker charter, and the provider rules have measured byte ceilings in `prompts::tests::prompt_diet`; a surface may shrink freely, but raising a ceiling is a decision.

## Related docs

- [Core essence](../../essences/core.md#general-agent) — General Agent lifecycle and behavior contract
- `prod-docs/MEMORY.md` — memory system including the behavioral protocol from `bridge_mem.md`
- [tiers.md](../../.agents/skills/prompts/tiers.md) — progressive-disclosure tiers and their evaluation boundary
- `.agents/skills/prompt-evaluator/SKILL.md` — the harness it drives
