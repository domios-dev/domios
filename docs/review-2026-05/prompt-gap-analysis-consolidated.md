# Prompt ↔ Code Gap Analysis — Consolidated (FINAL, rev 2)

Merge of **three independent passes** + a **fourth deeper verification pass** that
reads each cited code line personally. Findings cross-validated against the source;
the verification column says **what I confirmed with my own eyes**, not "another agent
said so".

| Pass | Agent | Source file | Date |
|------|-------|-------------|------|
| Claude P1 | (orchestrator, first sweep) | `docs/review-2026-05/prompt-gap-analysis.md` | 2026-05-30 |
| Codex / GPT-5.5 | Liskov | `docs/gap-analysis-prompts-2026-05-25.md` | 2026-05-25 |
| DeepSeek | Bhabha | `docs/gap-analysis-deepseek-2026-05-25.md` | 2026-05-25 |
| **Claude P2 (verification)** | this doc | line-by-line code reads | 2026-05-31 |

**Already fixed this session** (code + most docs): `AgentRole` collapsed to
`{Orchestrator, Worker}` (`types.rs:34`); `role.rs` ; parse sites (`types.rs:48`,
`snapshot.rs:142`, `service/mod.rs:188`); suffix selection (`orchestrator.rs:401`,
`spawn.rs:312`); `prompts.rs` LEAD_SUFFIX deleted; `lead_suffix.md` removed;
`unlink` now detaches Workers (`role.rs:57`). Doc/prompt Lead-removal covered:
`docs/{API,SKILLS,PROMPTS,ARCHITECTURE,contributing/agents,contributing/prompts}.md`,
`codeterm_cli.md`, `codeterm_rule.md`, `bootstrap_instructions.md`. Plus my own
G1/G2/G5/G6 fixes (F6/F17/F18/F19 below).

---

## Verified findings

Verify column:
**✓** code read confirms the claim · **✗** code read **invalidates** the claim ·
**∆** the situation is more severe / specific than originally stated · **design** = a
deliberate design call, not a bug · **?** still needs a read.

Found-by: **C** = Claude, **X** = Codex/Liskov, **D** = DeepSeek/Bhabha.

| # | Sev | Area | Finding | Verify | Found-by | Status |
|---|-----|------|---------|:------:|:--------:|:------:|
| **F1** | **P0** | `skills/codeterm/references/roles.md` + `SKILL.md` | **Live runtime skill trains agents on the deleted Lead role.** roles.md:8-23 redefines the 3-level hierarchy; :43-47 gives a `--role Lead --parent $PANE --can-spawn` template; :68-91 a whole "Lead" role section; :108-112 a Lead task template. Following any of these → backend rejects (`service/tests.rs:16-18`, `state/role.rs:31-48`). SKILL.md:3 also lists `Orchestrator/Lead/Worker`. **The most dangerous gap** because agents *load this at runtime*. | ✓ | X, D | 🔧 open |
| **F2** | **P0** | `CLAUDE.md:139-143` | Still: "4-level hierarchy: Orchestrator → Lead → Worker → SubWorker", with Lead spawning Workers (line 142) and Worker/SubWorker rules (143). | ✓ | X, D | 🔧 open |
| **F3** | **P0** | `codeterm_cli.md:56` | `codeterm agent model sonnet\|haiku\|opus` printed as if generic. CLI is honest: `args/agent.rs:74` doc *says* "Claude only", `cmd/agent.rs:275` errors "Codex /model is an interactive picker, not scriptable." → the prompt is the only place that misleads. | ✓ | X, D | 🔧 open |
| **F4** | **P0** | mesh `api_map.rs` + `codeterm_cli.md:105` | **"Any local command works via `--mesh`" is wrong, and the divergence is *worse* than the other passes caught.** Verified by reading the code: (a) `mem secret/flush/skill/worker/run-worker` all map to `GET /api/mem/stats` instead of erroring (`api_map.rs:669-674`). (b) `cron create` (`api_map.rs:184-191`) drops `once / jitter_max_ms / tz / timeout_secs / creator_pane`. (c) `graph entities create`: local sends `{"id","type","scope","properties"}` (`cmd/mem/mod.rs:457-459`), mesh sends `{"id","entity_type","props"}` (`api_map.rs:549`) — **two keys mismatch, not one** (`type`↔`entity_type` AND `properties`↔`props`). | ✓ ∆ | C, X, D | 🔧 open |
| **F5** | **P1** | `codeterm_cli.md:161` + docs/MEMORY/CONFIG + scheduler | **Jitter is additive only, not ±.** `cron/store/schedule.rs:88-102`: `max = clamp(interval/10, 60_000, 1_800_000)`; `offset = rng.gen_range(0..=max)`; **`next + offset`** — never `next - x`. A 6:00 cron with default jitter fires 6:00–6:30, never 5:30. The "±" wording is in ≥4 places. | ✓ | X, D | 🔧 open |
| **F6** | **P1** | `codeterm_rule.md:74` | Said default backend is "Bitwarden". Real default = `local` OS keychain (`config/schema/mod.rs:520`). | ✓ | C, X, D | ✅ fixed |
| **F7** | **P1** | `docs/PROMPTS.md` | Stale prompt map: dead `.claude/agents/*.md` source, wrong module paths/line refs, extraction numbers wrong on **two** counts: `auto_extract.rs:17-19` excludes ONLY `MemKind::Summary` (not the kind subset PROMPTS.md lists); truncation is **50 000** chars (`auto_extract.rs:27`), not 4 000. | ✓ | X, D | 🔧 open (the Lead-suffix rows already removed) |
| **F8** | **P1** | `docs/GENERAL_AGENT.md` | Dead spec / time-capsule. Most specific marker: `instructions.rs:185-186` **asserts in a test** that the generated template `!contains("CODETERM-AUTO-START")` and `!contains("CODETERM-AUTO-END")` — the markers the spec describes are *explicitly forbidden* by the code. EXTRA_INSTRUCTIONS, `{connected_services}`, Phase 0-6, secrets-as-md-files — same pattern, code never built them. | ✓ | D | 🔧 open |
| **F9** | P2 | `mem_extraction.md:65`, `dreaming_rem.md:362` | "`mem search` returns BOTH mem_items AND graph entities" reads as a guarantee; entity hits depend on embedding/index/query shape. | (design) | X, D | 🔧 open |
| **F10** | P2 | `bridge_mem.md` | "use the codeterm skill" is ambiguous — three surfaces with the same name: `seed/skill_codeterm.md` (16-line stub returned by `mem search`), `skills/codeterm/SKILL.md` (on-disk, `{codeterm_cli}` substituted), `codeterm_cli.md` (full ref). `mem search` returns the stub. | ✓ | C, X, D | 🔧 open |
| **F11** | — | `seed/skill_codeterm.md` (`mem worker spawn`) | DeepSeek claimed "advertised, not implemented." **INVALIDATED** by reading the code: `MemWorkerAction::Spawn { intent, task, agent, parent }` exists in `args/mem.rs`, **and the handler is wired** at `cli/src/cmd/mem/mod.rs:610`. Command runs. | ✗ | D | n/a |
| **F11b** | P2 | `seed/skill_codeterm.md` ("via Bash") | Says "use the CLI via Bash"; project guidance is PowerShell on Windows (`CLAUDE.md:54`). | ✓ | X, D | 🔧 open |
| **F12** | P2 | `bootstrap_instructions.md:69` | `rm bootstrap.md` is Unix-specific + path-relative. Cross-platform repo (Windows shells too). Use shell-neutral wording + `{general_dir}/bootstrap.md`. | ✓ | X, D | 🔧 open |
| **F13** | P2 | `worker_suffix.md` | One line ("run `codeterm agent report 'summary'`") — no schema, while `docs/contributing/agents.md` (and X/D) imply STATUS/CHANGES/BUILD/FOLLOWUP. Either enforce the template in the suffix or stop implying structure. | (design) | X, D | 🔧 open |
| **F14** | P2 | dreaming reporting | Three report channels (cron `complete`, cron creator `agent report`, hidden `mem worker report`). Document the lifecycle. | (design) | X, D | 🔧 open |
| **F15** | P3 | `codeterm_cli.md` docs list | `codeterm docs` embeds 11 docs (`cmd/docs.rs`); `MODELS.md`/`PLUGINS.md`/plan docs unreachable via CLI. Probably intentional. | ✓ | X | 🔧 open (minor) |
| **F16** | P3 | `codeterm_rule.md:61,70` | `echo -n "$V" \|` is bash; on Windows PowerShell `echo` adds a newline. Possible trailing-newline in saved secret. | ? | C | 🔧 open (verify) |
| **F17** | — | `codeterm_rule.md:26` | `send-text PANE_ID "command"` wrong arg order. | ✓ | C | ✅ fixed |
| **F18** | — | `codeterm_cli.md:47` | "use `batch`" — no `codeterm batch` CLI; REST/IPC only. | ✓ | C | ✅ fixed |
| **F19** | — | `codeterm_cli.md` | `codeterm usage` (token tracking) never surfaced. | ✓ | C | ✅ fixed (block added) |
| **F20** | **P1** | `codeterm_cli.md` ↔ `cli/src/{args,cmd}/plugin.rs` | **`codeterm plugin` is entirely undeclared to agents.** `cli/args/plugin.rs` defines `PluginAction { List, Install (id/from-dir/...), Update, UpdateSystem, Remove, Channel{Add/List/Refresh/Remove} }`. `cmd/plugin.rs` is **26 KB** of real implementation. `cli/args/cli.rs:24` after_help even points humans at it ("Plugin management is available via `codeterm plugin --help`") — but agents reading `codeterm_cli.md` see *zero* mentions of "plugin". | ✓ | C (P2) | 🔧 open |
| **F21** | P2 | `codeterm_cli.md` (mem section) | Several `mem` subcommands undocumented for agents despite being agent-useful: `mem learn / learn-list / learn-promote` (capture insights/errors/feature reqs — a designed-for-agents flow), `mem history`, `mem taxonomy`, `mem extract-pending`, `mem flush`, `mem reembed`. The bigger "lost feature" is the `learn` family — it has a dedicated UI (Memory panel) but agents are never taught the CLI verbs. | ✓ | C (P2) | 🔧 open |
| **F22** | P3 | `codeterm_cli.md` (agent section) | Missing minor: `agent providers` (list registered providers + default models + counts) and `agent models <provider>` (list models for one provider). Useful for an agent that needs to pick a model. | ✓ | C (P2) | 🔧 open (minor) |

---

## Cross-validation summary

- **Triple-confirmed and code-verified by me:** F4 (mesh — *more divergent* than X/D thought, plus `properties`↔`props`), F6 (secret default — already fixed).
- **Double-confirmed (X+D) and code-verified by me:** F1, F2, F3, F5, F7, F8, F10, F11b, F12, F13 (design), F14 (design), F15.
- **Invalidated by code read:** F11 (`mem worker spawn` is real). DeepSeek miss.
- **Single-source, code-verified:** F8 (DeepSeek) — `instructions.rs:185-186` asserts markers absent; that's the cleanest "dead spec" evidence in the audit.
- **My P2 additions from the deeper sweep:** F20 (`codeterm plugin` undeclared — biggest), F21 (`mem learn` family undeclared), F22 (`agent providers/models` undeclared minor).

The cross-model agreement on the P0 cluster is the strongest signal: **the dangerous
gaps are the stale Lead surfaces (especially the live skill F1) and the mesh
over-promise F4 — both verified by my own code reads.**

---

## Recommended remediation order (post-verification)

1. **F1 — `skills/codeterm/references/roles.md` + `SKILL.md`** → 2-level. **TOP priority** (this is the live runtime skill; following it gets the agent's commands rejected by the backend).
2. **F2 — `CLAUDE.md:139-143`** → 2-level (decide whether it's hand-maintained or regenerated; the rule says "never edit CLAUDE.md" but its content is clearly stale).
3. **F4 — mesh:** (a) prompt: narrow to a supported list; (b) code: make unsupported mesh paths fail loudly (not /stats); (c) fix `entity_type`↔`type` AND `properties`↔`props` mismatch in `api_map.rs:549`; (d) pass through the dropped cron fields in `api_map.rs:184`.
4. **F3 — `codeterm_cli.md:56`** — label `agent model` Claude-only; add the respawn recipe for Codex.
5. **F5 — jitter wording** in `codeterm_cli.md`, `docs/MEMORY.md`, `docs/CONFIG.md`, `mem/dreaming.rs:689` comment → "0..max delay" (or change the scheduler to truly ±).
6. **F20 — add a `## Plugins` block** to `codeterm_cli.md` covering `plugin list/install/update/remove + channel`. (Currently agents can't install or even *list* plugins via CLI even though the surface is rich.)
7. **F7 — rewrite `docs/PROMPTS.md`** from `prompts.rs` + current call sites; drop `.claude/agents`; fix extraction numbers (50 000 truncate; gate = `!skip_extract`; kind exclusion is `Summary` only).
8. **F8 — `docs/GENERAL_AGENT.md`** → rewrite to reality or clearly mark as historical/scrap.
9. **F21 — `mem learn`** block in `codeterm_cli.md` (small).
10. **F9–F14 + F22** — clarity pass + minor declarations.

> Already done: F6, F17, F18, F19. Prompt edits are `include_str!`'d → live on next `./build.sh dev`.

<!-- revision: 2 -->
