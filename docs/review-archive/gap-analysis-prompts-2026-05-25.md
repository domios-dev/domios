Historical snapshot (2026-05-25). Findings may since be fixed; do not action without re-verifying.



# Prompt Gap Analysis - 2026-05-25

Scope: all owned agent-facing prompt files under `src-tauri/prompts/`, the
`skills/codeterm` skill, and the docs that describe how those prompts and agent
commands work. The audit checks prompt claims against the Rust/CLI code that
consumes them.

## Executive Summary

The highest-risk gap is not one bad line; it is drift between three generations
of the system:

- Current code implements a flat `Orchestrator -> Worker` hierarchy.
- Several prompts/docs still describe `Lead` and `SubWorker`.
- Mesh and cron prompts state broad contracts that are narrower in code.
- Prompt documentation still points at old module paths, missing files, and old
  defaults.

Agents reading the stale prompts can issue commands that the backend rejects
(`--role Lead`, Worker spawning, Codex model switching), misunderstand scheduling
jitter, or assume remote mesh support that silently maps to weaker endpoints.

## Prompt Inventory

| Source | Runtime consumer | Status |
|---|---|---|
| `general_agent.md` | `service/general_agent/instructions.rs` writes `AGENT.md`; `AGENTS.md` bundles it with `codeterm_rule.md` and `bridge_mem.md` | Mostly aligned; docs around it are stale |
| `codeterm_cli.md` | Included into `general_agent.md` and codeterm skill content | Several command-contract gaps |
| `bootstrap_instructions.md` | Written once to `bootstrap.md`; fresh GA init tells the agent to read it | Works, but cross-platform command wording is weak |
| `codeterm_rule.md` | Claude rule + Codex `AGENTS.md` bundle | One stale secrets default |
| `bridge_mem.md` | Claude rule + Codex `AGENTS.md` bundle | Mostly aligned; "use codeterm skill" is ambiguous |
| `worker_suffix.md` | Appended for spawned role `Worker` in `service/spawn.rs` and `service/orchestrator.rs` | Aligned, but minimal |
| `orchestrator_suffix.md` | Appended for spawned role `Orchestrator` | Aligned |
| `cron_report_hint.md` | Appended when cron has creator pane | Aligned |
| `cron_complete_suffix.md` | Always appended to cron task text | Aligned |
| `worker_task_preamble.md` | Wraps headless memory workers | Aligned |
| `mem_extraction.md` | Inner task for auto-extraction worker | Mostly aligned; docs about trigger/truncation are stale |
| `dreaming_rem.md` | Inner task for REM worker | Mostly aligned; docs overstate +/- jitter |
| `mem_rerank.md` | Reranker prompt | No material gap found in this pass |
| Seed prompts in `src-tauri/prompts/seed/` | Seeded memory facts/rules/skill | Some stale/overbroad command wording |
| `skills/codeterm/SKILL.md` and references | Codex/Claude skill surface | Role reference is stale |

## Findings

### P0 - Agent hierarchy prompts still advertise deleted roles

**Claim:**

- `CLAUDE.md:139` says hierarchy is `Orchestrator -> Lead -> Worker -> SubWorker`.
- `skills/codeterm/SKILL.md:3` describes `(Orchestrator/Lead/Worker)`.
- `skills/codeterm/references/roles.md` documents Lead spawning Workers and
  examples using `--role Lead`.

**Code reality:**

- `src-tauri/src/multiplexer/types.rs:34` defines only `AgentRole::Orchestrator`
  and `AgentRole::Worker`.
- `src-tauri/src/service/tests.rs:16-18` explicitly asserts `Lead` and
  `SubWorker` are rejected.
- `src-tauri/src/multiplexer/state/role.rs:37` requires a Worker's parent to be
  an Orchestrator.
- `src-tauri/src/multiplexer/state/role.rs:189-196` rejects
  `set_can_spawn(true)` on a Worker.
- `docs/contributing/agents.md:3` correctly documents the current 2-level tree.

**Impact:** Agents can be instructed to spawn Leads or nested Workers, which the
backend rejects. This is a hard prompt/code contradiction.

**Fix:** Remove Lead/SubWorker from `CLAUDE.md` and
`skills/codeterm/references/roles.md`, or reintroduce those roles in code. Since
the code and tests intentionally deleted them, the docs/prompts should be
changed to the 2-level contract.

### P0 - `docs/PROMPTS.md` describes non-existent prompt locations

**Claim:**

- `docs/PROMPTS.md:9-12` says prompts live in exactly two places:
  `src-tauri/prompts/*.md` and `.claude/agents/*.md`.
- `docs/PROMPTS.md:24,112-113,129,141` repeats `.claude/agents/*.md` as an
  active prompt location.

**Code/repo reality:**

- `.claude/agents/` does not exist in this checkout.
- `CLAUDE.md:145-147` and `docs/contributing/agents.md:12-14` say role skills
  intentionally moved to `codeterm mem`.
- Runtime prompt files are in `src-tauri/prompts/`; provider rules are also
  generated into `~/.claude/rules/` or bundled into Codex `AGENTS.md`.

**Impact:** Contributors looking for prompt sources are sent to a dead path and
may miss the actual mem-backed role guidance.

**Fix:** Rewrite `docs/PROMPTS.md` around current sources:
`src-tauri/prompts/`, `skills/codeterm/`, mem seed prompts, and generated
provider wrappers. Remove `.claude/agents` as an active source unless it is
reintroduced.

### P0 - `lead_suffix.md` / `LEAD_SUFFIX` references are stale

**Claim:** Older prompt docs referenced `lead_suffix.md` / `LEAD_SUFFIX`.

**Code reality:**

- `src-tauri/src/prompts.rs:7-10` exposes only `WORKER_SUFFIX` and
  `ORCHESTRATOR_SUFFIX` for role suffixes.
- `src-tauri/src/service/spawn.rs:311-318` and
  `src-tauri/src/service/orchestrator.rs:400-402` only append suffixes for
  `Worker` and `Orchestrator`.
- `git status` currently shows `D src-tauri/prompts/lead_suffix.md`.

**Impact:** Any doc or agent flow expecting a Lead suffix is impossible.

**Fix:** Same as hierarchy fix: remove the Lead suffix concept from docs/skills,
or rebuild Lead support end to end.

### P1 - Secret backend default is wrong in `codeterm_rule.md`

**Claim:** `src-tauri/prompts/codeterm_rule.md:74` says the default backend is
Bitwarden.

**Code/docs reality:**

- `src-tauri/src/config/schema/mod.rs:508` has `default_secrets_backend()`.
- `src-tauri/src/config/schema/mod.rs:553` uses that default in
  `SecretsConfig`.
- `docs/API.md:493`, `docs/CONFIG.md:274`, and `src-tauri/prompts/general_agent.md:52`
  say the default is local OS keychain.

**Impact:** Agents may try to use or configure Bitwarden unnecessarily on fresh
installs.

**Fix:** Change the rule to: default is Local OS keychain; Bitwarden/Vaultwarden
is optional when configured.

### P1 - Mesh prompt says "any local command" but mesh relay is partial

**Claim:** `src-tauri/prompts/codeterm_cli.md:105` says any local command works
remotely by adding `--mesh TARGET`.

**Code reality:**

- `cli/src/mesh_cli/mod.rs:143` rejects mesh management commands as local-only.
- `docs/API.md:493` documents `mem secret` as local-only; mesh rejects those.
- `cli/src/mesh_cli/api_map.rs:668-672` maps `mem flush`, `run-worker`,
  `skill`, `worker`, and `secret` to `GET /api/mem/stats` instead of their real
  local behavior.
- `cli/src/mesh_cli/api_map.rs:184-192` maps `cron create` but drops fields
  present in local CLI handling (`once`, `jitter_max_ms`, `tz`, `timeout_secs`,
  `creator_pane`).
- `cli/src/mesh_cli/api_map.rs:546-549` uses graph create payload key
  `entity_type`, while local code at `cli/src/cmd/mem/mod.rs:454-458` sends
  `"type"`.

**Impact:** Agents can believe a remote command worked when it actually hit a
stats endpoint or lost important options. The broad prompt is unsafe.

**Fix:** Replace "any local command works" with a supported-command list.
Unsupported mesh paths should fail loudly instead of mapping to stats.

### P1 - Cron jitter is documented as +/- but implemented as delay-only

**Claim:** `src-tauri/prompts/codeterm_cli.md:161` says default jitter is
`+/-10% of interval`. `docs/MEMORY.md:18,227`, `docs/CONFIG.md:436`, and
`src-tauri/src/mem/dreaming.rs:689` use similar +/- wording for dreaming.

**Code reality:**

- `src-tauri/src/cron/store/schedule.rs:88-101` computes a non-negative max.
- `src-tauri/src/cron/store/schedule.rs:101` picks `gen_range(0..=max)`.
- The returned value is `next + offset`, never before the scheduled time.
- `docs/API.md:435` and `docs/CONFIG.md:381` correctly say "max random delay
  added".

**Impact:** Agents and users expect jobs to fire before or after the nominal
time; code only delays them. For a 6:00 dreaming cron with 30-minute jitter,
the real window is 6:00-6:30, not 5:30-6:30.

**Fix:** Update prompt/docs wording to "0..max delay" or change scheduler logic
to truly support +/- windows.

### P1 - Codex model switching is over-advertised

**Claim:** `src-tauri/prompts/codeterm_cli.md:56` lists
`codeterm agent model sonnet|haiku|opus [--pane ID]` without provider scoping.

**Code reality:**

- `cli/src/args/agent.rs:74` says this is Claude-only.
- `cli/src/cmd/agent.rs:275` prints "Codex /model is an interactive picker, not
  scriptable."
- `src-tauri/src/plugin/agent_codex/provider.rs:373` leaves model switching
  unsupported.

**Impact:** Codex agents may try a command that is guaranteed to fail.

**Fix:** In `codeterm_cli.md`, say `agent model` is Claude-only and Codex model
changes require respawn with `agent spawn codex --model ...`.

### P1 - `docs/PROMPTS.md` has stale code paths and line references

**Claim:** `docs/PROMPTS.md:32-43` points at old flat modules such as
`service/general_agent.rs:114` and `commands.rs:707`.

**Code reality:**

- General agent prompt generation lives in
  `src-tauri/src/service/general_agent/instructions.rs:127-158`.
- Claude provider rule integration is under plugin provider code, not
  `commands.rs:707`.
- `CRON_REPORT_HINT` / `CRON_COMPLETE_SUFFIX` are appended in
  `src-tauri/src/cron/scheduler.rs:154-156` and
  `src-tauri/src/service/cron.rs:171-173`.

**Impact:** The doc is not reliable for prompt ownership or review.

**Fix:** Regenerate the table from `src-tauri/src/prompts.rs` and the current
call sites.

### P1 - Memory extraction docs have stale trigger and truncation details

**Claim:** `docs/PROMPTS.md:59` says auto-extraction is for items longer than
about 80 chars and the body is truncated to 4000 chars.

**Code reality:**

- `src-tauri/src/mem/auto_extract.rs:27-42` truncates at 50,000 bytes/chars
  boundary, not 4000.
- `src-tauri/src/api/mem_routes/items.rs:415` gates extraction with
  `!skip_extract`, not with the doc's visible 80-char threshold.
- `src-tauri/src/mem/auto_extract.rs` skips only `Summary` via `should_extract`;
  the doc's kind list is not the full source of truth.

**Impact:** Prompt reviewers and future maintainers will tune the wrong budget
and wrong trigger model.

**Fix:** Update docs to the current trigger path and 50,000 truncation.

### P1 - `skills/codeterm/references/roles.md` can actively train agents wrong

This is separate from the generic hierarchy gap because the skill is what agents
may actually read during task execution.

**Claim:** The file says Leads can spawn Workers, Workers can have Lead parents,
and gives templates for Lead tasks.

**Code reality:** Same two-level code evidence as P0.

**Impact:** The skill is a live prompt surface, not just docs. It should be
treated as prompt code and fixed before relying on autonomous delegation.

**Fix:** Replace the file with the current Orchestrator/Worker rules from
`docs/contributing/agents.md`.

### P2 - `bridge_mem.md` says "use the codeterm skill" without saying which one

**Claim:** `bridge_mem.md` redirects agents to "the codeterm skill" for the full
command reference.

**Reality:** There are multiple surfaces named codeterm:

- `src-tauri/prompts/seed/skill_codeterm.md` - short mem seed stub.
- `skills/codeterm/SKILL.md` - on-disk skill with `{codeterm_cli}` substitution.
- `src-tauri/prompts/codeterm_cli.md` - the actual large command reference.

**Impact:** Agents may search mem and get the short seed when the full reference
was intended.

**Fix:** Say "use the `codeterm` skill; if unavailable, run
`codeterm docs API` / read the injected CodeTerm CLI Reference" or consolidate
the seed stub with the full skill.

### P2 - Seed skill says "via Bash"; project guidance says PowerShell on Windows

**Claim:** `src-tauri/prompts/seed/skill_codeterm.md:16` says to always use the
CLI from inside CodeTerm via Bash.

**Repo guidance reality:** `CLAUDE.md:54` says this project should run cargo via
PowerShell foreground, not Bash, because Bash can orphan long-running commands.

**Impact:** The prompt is over-specific and can create Windows/dev workflow
bugs.

**Fix:** Say "use the CodeTerm CLI from the current shell; do not call REST
directly unless implementing/debugging CodeTerm itself."

### P2 - Bootstrap deletion command is Unix-specific

**Claim:** `src-tauri/prompts/bootstrap_instructions.md:69` says
`rm bootstrap.md`.

**Code reality:**

- `src-tauri/src/service/general_agent/instructions.rs:156-158` writes the file.
- `src-tauri/src/service/general_agent/lifecycle.rs:389` tells the GA to check
  it on first boot.
- The shell can be PowerShell/CMD/Bash depending on platform/provider.

**Impact:** On Windows, `rm` may be an alias in PowerShell but is not portable
across all shells.

**Fix:** Use shell-neutral wording: delete `{general_dir}/bootstrap.md`, with
PowerShell and POSIX examples.

### P2 - Worker report format is minimal, while docs imply richer enforcement

**Claim:** Some docs imply structured Worker reports.

**Code/prompt reality:**

- `src-tauri/prompts/worker_suffix.md:2` only says run
  `codeterm agent report "your summary"`.
- There is no enforced schema in `src-tauri/src/agent/reports.rs`.

**Impact:** Orchestrators cannot reliably parse Worker results.

**Fix:** Either add a structured report template to `worker_suffix.md`, or stop
claiming structure exists.

### P2 - Dreaming cron completion has layered reporting semantics

**Claim/code mix:**

- `src-tauri/src/mem/dreaming.rs:685` creates a cron task:
  `codeterm mem dream && codeterm agent report "dreaming complete"`.
- The cron runner appends `CRON_COMPLETE_SUFFIX` at
  `src-tauri/src/cron/scheduler.rs:156`.
- `dreaming_rem.md:114,647` tells the hidden REM worker to run
  `codeterm mem worker report`.

**Reality:** This is probably intentional layering:

- cron agent reports to creator with `agent report` if applicable,
- cron run is closed with `cron complete`,
- hidden mem worker reports through `mem worker report`.

**Gap:** The docs should explicitly explain the three channels. Right now this
looks contradictory when reviewing prompts, and it is easy to add a fourth
duplicate report by accident.

### P2 - `mem search returns BOTH mem_items AND graph entities` is too absolute

**Claim:** `mem_extraction.md:65` and `dreaming_rem.md:362` say `mem search`
returns both mem items and graph entities.

**Code reality:** Entity hits are supported, including entity-kind results in
search paths, but quality depends on entity vectors / graph-aware indexing and
query shape. The prompt's wording reads like a guarantee that every relevant
entity will appear in every `mem search`.

**Impact:** Workers may skip explicit graph commands when search misses an
entity.

**Fix:** Rephrase as "mem search can return graph entities; for exact entity
checks use graph/get/repair commands where appropriate."

### P3 - `codeterm docs` prompt list is accurate but incomplete for project docs

**Claim:** `codeterm_cli.md` lists the embedded docs.

**Code reality:** `cli/src/cmd/docs.rs` embeds exactly:
`CONFIG`, `API`, `MESH`, `MEMORY`, `BUILD`, `PROMPTS`, `SKILLS`, `TESTING`,
`TELEGRAM-SETUP`, `SECRETS`, `TEXT-HOOKS`.

**Gap:** This is accurate for CLI, but not a full docs index. Files like
`docs/MODELS.md`, `docs/PLUGINS.md`, and plan docs are not available through
`codeterm docs`.

**Fix:** No prompt bug if intentional. If agents are expected to discover plugin
or model docs via CLI, add them to `DOCS`.

## Prompt-by-Prompt Notes

### `general_agent.md`

No critical contradiction found. The secrets line is more accurate than
`codeterm_rule.md` because it says Local is the default. The lifecycle claim that
sub-pane reports are bridged by GA aligns with the report/bridge architecture.

Risk: It depends on `codeterm_cli.md`, so command-contract errors there are
inherited by the GA.

### `codeterm_cli.md`

Main gaps:

- `agent model` should be labeled Claude-only.
- Mesh support is overstated.
- Cron jitter wording should be delay-only, not +/-.
- Remote cron create limitations are not called out.

Good parts:

- It correctly warns that commands default to focused pane, not necessarily
  `$CODETERM_PANE_ID`.
- It correctly emphasizes `agent report` for sub-panes.
- It correctly says secrets belong in `mem secret`, not `mem save`.

### `codeterm_rule.md`

Main gap: stale Bitwarden default. Otherwise its worker reporting and
auto-generated-file warnings match the current system.

### `bridge_mem.md`

Mostly aligned with current memory rules. The ambiguous "codeterm skill"
reference should be tightened.

### `worker_suffix.md` / `orchestrator_suffix.md`

Both align with code paths. The Worker suffix may be too weak if structured
reports are desired, but it does not lie about current behavior.

### Cron prompts

`cron_report_hint.md` and `cron_complete_suffix.md` align with code. The problem
is in surrounding docs and CLI reference wording about jitter semantics.

### Memory worker prompts

`worker_task_preamble.md`, `mem_extraction.md`, `dreaming_rem.md`, and
`mem_rerank.md` are mostly coherent. The stale parts are in `docs/PROMPTS.md`
and in absolute wording around `mem search` entity behavior.

### Seed prompts

`rule_never_edit_autogen.md`, `rule_save_to_mem.md`, and
`rule_search_before_asking.md` match the current intended behavior.

`skill_codeterm.md` is too small and says "via Bash"; it should either be a
short pointer to the real skill or use shell-neutral wording.

## Recommended Fix Order

1. Remove Lead/SubWorker from live skill/prompt surfaces:
   `CLAUDE.md`, `skills/codeterm/SKILL.md`, and
   `skills/codeterm/references/roles.md`.
2. Rewrite `docs/PROMPTS.md` from `src-tauri/src/prompts.rs` and current call
   sites.
3. Patch `codeterm_cli.md` for mesh, jitter, and Claude-only model switching.
4. Patch `codeterm_rule.md` and seed skill for secrets default / shell wording.
5. Decide whether mesh unsupported commands should error instead of mapping to
   stats.
6. Decide whether Worker reports need a structured schema; if yes, put it in
   `worker_suffix.md` and tests.

## Codex Personal Findings

This section is my independent pass over the prompt/code gaps, intended for an
orchestrator to merge with other agents' reports.

### Strongest confirmed gaps

1. **The live agent hierarchy is two-level, but several prompt surfaces still
   teach a multi-level hierarchy.** This is the most dangerous gap because it
   directly causes rejected commands. Code and tests are explicit:
   `AgentRole` has only `Orchestrator` and `Worker`; `Lead` and `SubWorker` are
   rejected. The stale surfaces are `CLAUDE.md` and
   `skills/codeterm/references/roles.md`.

2. **`docs/PROMPTS.md` is no longer trustworthy as the prompt map.** It still
   names `.claude/agents` as an active prompt source, uses old module paths, and
   refers to prompt artifacts that no longer exist. Any future prompt work based
   on that doc risks editing the wrong place.

3. **`codeterm_cli.md` overstates mesh support.** The prompt says any local
   command works via `--mesh`, but the mesh CLI implementation rejects some
   commands, drops fields on others, and maps several unsupported memory actions
   to `/api/mem/stats`. This should be narrowed in the prompt and hardened in
   code.

4. **Cron jitter wording is semantically wrong.** The prompt/docs use `+/-`
   language, but the scheduler only adds a non-negative delay. This changes the
   user's expected execution window, especially for dreaming.

5. **Provider-specific model switching is under-specified.** The prompt lists
   `codeterm agent model sonnet|haiku|opus` as if it is a generic command, while
   the CLI and Codex provider explicitly say Codex `/model` is interactive and
   not scriptable.

6. **Secrets default drift exists inside prompts.** `general_agent.md` and docs
   say Local OS keychain is default, matching code; `codeterm_rule.md` still says
   Bitwarden is default. This should be fixed because agents may otherwise try
   to set up the wrong backend.

### Lower-confidence or design gaps

1. **`mem search returns BOTH mem_items AND graph entities` is technically too
   absolute.** Code supports entity hits, but the prompt reads like a guarantee
   that every relevant entity appears through normal search. I would soften the
   language and explicitly tell workers to use graph commands for exact entity
   checks.

2. **Dreaming has three reporting channels that look contradictory during
   prompt review.** Cron completion, cron creator reports, and hidden memory
   worker reports are probably intentional, but the docs should explain the
   lifecycle to prevent accidental duplicate reporting.

3. **The codeterm skill name is overloaded.** There is a mem seed stub, an
   on-disk skill, and the injected `codeterm_cli.md` reference. Prompts that say
   "use the codeterm skill" should point to the exact surface or fallback command
   because agents can otherwise read the short stub and miss the full contract.

### Recommended first patch set

1. Patch `CLAUDE.md` and `skills/codeterm/references/roles.md` to the current
   `Orchestrator -> Worker` model.
2. Patch `codeterm_cli.md` for `--mesh`, cron jitter, and Claude-only
   `agent model`.
3. Patch `codeterm_rule.md` to Local-by-default secrets.
4. Rewrite `docs/PROMPTS.md` after those live prompt fixes, so the doc becomes a
   reliable map again instead of preserving stale concepts.

## Checks Performed

- Searched prompt/documentation files with `rg`.
- Cross-checked role handling in `multiplexer/types.rs`, `state/role.rs`,
  `service/spawn.rs`, `service/orchestrator.rs`, and role tests.
- Cross-checked CLI mesh mappings in `cli/src/mesh_cli/api_map.rs`.
- Cross-checked cron jitter implementation in
  `src-tauri/src/cron/store/schedule.rs`.
- Cross-checked prompt includes in `src-tauri/src/prompts.rs`.
- Cross-checked embedded docs list in `cli/src/cmd/docs.rs`.

This document is a gap analysis only; it does not apply the prompt fixes.

