---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_553bdf825b194a2797e54abdd6eb3b94
handle: planner-role-design
title: Planner role — adopted design (1.6.2)
---

# Planner role — adopted design (1.6.2)

## Decision

**Planner is a behavioral PROFILE on the Worker structural role, not a third `AgentRole` variant.** Two-layer role model:

- **Layer 1 — structural (closed enum, unchanged):** `AgentRole = {Orchestrator, Worker}` keeps owning hierarchy semantics: parent link, `can_spawn`, depth, report routing, reconcile, presence. A Planner IS structurally a Worker: spawned by an Orchestrator, parent-linked, `can_spawn=false`, one bounded task, reports `in_review`, exits.
- **Layer 2 — behavioral (open string):** `role_profile: Option<String>` on the spawn spec + pane. It selects the onboarding prompt suffix, default model routing, and the presence/tree label. `"planner"` is the first (and for 1.6.2 the only) named profile. Worker-with-no-profile stays exactly as today — generic.

CLI sugar: `codeterm agent spawn claude --model fable --parent <orch-pane> --role Planner --task-file <brief>` maps at the CLI/route boundary to `role=Worker + role_profile="planner"`. The service-layer `parse_role` stays two-variant; the REST/RPC API carries `role` and `role_profile` as separate honest fields.

### Why not `AgentRole::Planner`

Every structural consumer (`multiplexer/state/role.rs`, snapshot/restore, closed store, `plan_reconcile`, hierarchy, remote workers, presence) would need a third match arm with semantics identical to Worker — pure duplication with no behavioral payoff. The real differences — prompt, model, label — are all Layer-2 concerns. `from_str_compat`'s `"Conductor"` alias is precedent for boundary-level aliasing without enum growth.

## Contract (who owns what)

- **Orchestrator** owns execution, worker reports, gates, and acceptance — unchanged.
- **Planner** is invoked by the Orchestrator to author/refine, IN the CodeTerm planner (source of truth, never markdown): GOAL (`plan goal set`), Spec sections (WHAT/WHY), Plan sections (HOW, spec-linked), Tasks (body + acceptance criteria + test plan + correctness + model routing + role guidance), and wave sequencing with file-ownership boundaries.
- **Planner does NOT:** spawn agents, assign panes, transition execution statuses, execute implementation work, or accept/close tasks. Asked to → report blocked.
- **Handback:** run `codeterm plan doctor <plan> --kind quality`, fix findings, read back writes (`plan get`/`plan state`), then RUN `codeterm agent report --plan <plan> --task <task> --status in_review` summarizing structural changes + open questions. The Orchestrator reviews the structure and dispatches the task graph.

## Model routing — Fable availability (command evidence)

- `codeterm agent models claude` lists `fable — Claude Fable 5`.
- End-to-end: `claude --model fable -p "Reply with exactly: FABLE-OK"` → `FABLE-OK` (this machine, 2026-07-02).
- The catalog seed (`plugin/agent_claude/provider/schemas.rs`) and live cache (`~/.codeterm/models/claude.cache.json`) mark fable `available: false` — **display metadata only**: `model_catalog.rs::resolve_model`/`find_model` match by id and never block on the flag, so `agent spawn claude --model fable` passes `--model fable` through today.
- Routing rule: Planner default `fable`, fallback `opus` (catalog fallback notice already exists). Orchestrator checks `codeterm usage latest --provider claude` headroom before spawning a Planner.

## Task-level role guidance

New free-form `role_guidance: Option<String>` on `Task` (plan/types.rs). Authored by the Planner (or Orchestrator) at `plan task add/update --role-guidance "..."` (+ `--role-guidance-file`); printed by `task show`; **consumed at dispatch**: when the Orchestrator spawns/respawns a worker for the task, the guidance is prepended to the brief (same composition point as the worker preamble). This is prompt-material, per-task, versioned with the plan — it replaces ad-hoc inline role text without inventing a role-template registry. (`taken_by_role` remains what it is: who took the task.)

## Mechanics — what changes where

1. `Pane.role_profile: Option<String>` — `multiplexer/types.rs` + snapshot + closed store (serde default; no migration, no external users).
2. Spawn: `role_profile` on the agent spawn spec + REST/RPC; CLI `--role Planner` sugar; unknown profile → warn + proceed as generic Worker.
3. Prompt: `src-tauri/prompts/planner_suffix.md` + `prompts.rs` const; `role_onboarding_task(task, role, profile)` picks `PLANNER_SUFFIX` for (Worker, "planner").
4. Task DTO `role_guidance` + CLI flags + dispatch prepend.
5. `TaskExecutionAttempt.role` (existing free-form field) records `"planner"`.
6. UI (minimal for 1.6.2): profile text next to the existing role badge in the agent tree/presence; task detail renders `role_guidance`. Nothing else.
7. Docs: `docs/ORCHESTRATION.md` Planner section; `docs/contributing/agents.md` role model; orchestrator onboarding + `orchestrator_suffix.md` teach "spawn a Planner".

## Planner onboarding prompt (draft for planner_suffix.md)

> You are the PLANNER — a high-reasoning role invoked by an Orchestrator to author or refine an effort's structure. The three Worker rules still bind you: one task, never spawn agents, report and exit.
>
> Scope — you author STRUCTURE; the Orchestrator executes it:
> - Read `codeterm docs ORCHESTRATION` first. The CodeTerm planner is the source of truth; never deliver a markdown plan as the artifact.
> - Author top-down: GOAL (`plan goal set`) → Spec sections (WHAT/WHY) → Plan sections (HOW, linked to spec) → Tasks.
> - Every task you author carries: body, section links, acceptance criteria, test plan, correctness vector, model routing (`--model` + `--why`), and role guidance (`--role-guidance`) the Orchestrator prepends when spawning that worker.
> - Sequence explicitly: waves, dependencies, and file-ownership boundaries so parallel lanes never collide.
>
> You do NOT: spawn agents, assign panes, change execution statuses, execute implementation work, or accept/close tasks. If asked to, report blocked.
>
> Before handing back: run `codeterm plan doctor <plan> --kind quality` and fix findings; read back what you wrote (`plan get` / `plan state`) to verify the writes landed.
>
> Hand back by RUNNING `codeterm agent report --plan <plan> --task <task> --status in_review "STRUCTURE: <sections/tasks added or changed> / DOCTOR: <result> / OPEN QUESTIONS: <...>"` — a CLI command, not chat text.

## Non-goals (explicitly NOT built now)

- **No persistent arbitrary custom role-template registry** — no role store, no `.claude/agents`-style files, no per-role mem entries. Profiles are a code-known set; `role_guidance` covers ad-hoc per-task needs.
- No third structural `AgentRole` variant; hierarchy/depth/can_spawn semantics untouched.
- No ACL enforcement of plan mutations by pane role — write discipline stays prompt-level (any pane with CLI access can mutate, as today).
- No system-initiated Planner (heartbeats never auto-spawn one); explicit Orchestrator spawn only.
- No dedicated Planner UI authoring surface.
- No hard model pinning; never block a spawn on the catalog `available` flag.

## Risks

- **Open-vocabulary profile typos** silently onboard as generic Worker → warn at spawn + a parity test for the known set.
- **Concurrent structural writes** (Planner + Orchestrator on one plan) hit the known locked-write-still-commits class → Planner batches edits and read-back-verifies; Orchestrator pauses its own structural edits while a planner lane is active.
- **Fable quota** — premium tier; usage check before spawn; fallback opus.
- **Stale catalog metadata** (`available:false` for fable in seed + cache) may gray fable out in UI pickers — cosmetic; fix seed/discovery as a follow-up nit, not a blocker.

## Sequencing (implementation lanes)

Run **sequentially** — both touch `src-tauri/prompts/` and onboarding text; parallel lanes would collide:

1. **task_216e66dfebef4bd4ade8012250c546d3** (role-aware Planner spawn/onboarding flow) — all src-tauri code: pane field, spawn/CLI sugar, `planner_suffix.md` + const + `role_onboarding_task` wiring, `role_guidance` DTO/CLI/dispatch, attempt recording, tests.
2. **task_ffddc406a8864e86804e60e6fd4ab45f** (prompts + orchestration docs) — refine prompt copy, `docs/ORCHESTRATION.md` + `docs/contributing/agents.md` + orchestrator onboarding mentions, revision bumps, eval scenario (cold planner uses planner CLI + hands back correctly).
