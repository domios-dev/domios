# Orchestration De-burring — CodeTerm changes that let the orchestrator skill drop its hacks

> Source: cross-machine recon by the rollacode (Mac-Air) orchestrator + a Codex
> reviewer, against the `skill_multi_agent_orchestration_pattern...` mem skill.
> Anchors below VERIFIED against codeterm-alpha tree on 2026-06-19 (line numbers
> drifted slightly from the rollacode checkout; alpha values recorded here).
> Owner: this orchestrator. Branch off `plugins-rethink`.

**Thesis:** several orchestration-skill workarounds exist only because CodeTerm
forces them, and the machinery to remove them mostly already exists. Fix the
code → the skill becomes a clean, general usage playbook.

---

## Status / tracked tasks

| Task | What | Owner | Status |
|------|------|-------|--------|
| P1 | Native worker-death watchdog | worker (Opus) | ✅ MERGED (f3f81077) |
| P4 | `agent spawn --task-file`/stdin | worker (Opus) | ✅ MERGED (b33b3e25) |
| P2 | Structured `agent report` | worker (Opus) | ✅ MERGED (acb40d6c) |
| P3 | Durable sqlite ledger + persist spawned_by | worker (Opus) | ✅ MERGED (911c768d), adversarial-reviewed SAFE |
| **SR** | **Rewrite the orchestration skill** (extract 2 dev-facts → mem, cut the leakage, apply the 9 flaw-fixes + new 9-section structure + locked spawn recipe + demote-not-delete heartbeat) | **Dante (this orchestrator)** | **⏳ IN PROGRESS — unblocked (all 4 code tracks landed). Fact files drafted.** |
| P5 | Optional `parity_test.rs` for `is_agent_pane` (native vs JS) | Dante | ⏳ queued (optional) |
| FU1 | `PRAGMA user_version` migration ladder for the ledger (before any future schema change) | Dante | ⏳ queued (non-blocking follow-up) |

All four code tracks are merged into `plugins-rethink` (currently @ `cfa9a829`).
SR is the remaining headline deliverable — it is the WHOLE POINT (fix the code so
the skill can drop its hacks, then actually rewrite the skill). **SR is assigned
to Dante** and done directly via `codeterm mem` (skill + facts live in mem, not
files), not delegated to a worker.

### SR sub-steps (Dante)
1. Save `fact_codeterm_plugin_runtime_sharp_edges` + `fact_codeterm_dev_push_hooks`
   to mem (kind=fact, area=codeterm) — the leakage extracted from the skill.
2. Rewrite the skill body: cut the 4 MECHANICS dev bullets (now in the facts),
   apply the 9 flaw-fixes, the new 9-section structure, the locked spawn recipe,
   and demote-not-delete for the heartbeat.
3. Save via `codeterm mem save --kind skill --replace` against the existing id
   `skill_multi_agent_orchestration_pattern_codeterm_orchestrator_per` (upsert in
   place; old version stays in mem history).
4. Verify recall (`codeterm mem search`) surfaces the rewritten skill.

---

## Code changes (ranked by skill-simplification × feasibility)

### P1 — Native worker-death watchdog (kills the 25-min heartbeat self-send). HIGH/HIGH
**Branch `orch-p1-death-watchdog`.** Role: Rust backend.

Today the daemon detects pane death (`pty_lifecycle.rs:196 handle_pty_death` →
`agent_tracker.set_dead` :198 → `pane-status-changed{Dead}`) and auto-fails a
CRON run on silent death (`pty_lifecycle.rs:210`). **Gap:** on a *hierarchy*
worker death nothing is injected into the parent pane, so the orchestrator only
learns by polling `agent reports` — which is exactly why the skill invented the
self-send heartbeat.

**Change:** in `handle_pty_death`, mirror the cron fallback — if the dying pane
has a `spawned_by_pane_id` / `report_target_pane_id` and emitted no terminal
report, synthesize one and route via `hierarchy::submit_report`
(`hierarchy.rs:45`; routing chain parent→report_target→spawned_by at :195-211):
`-=-codeterm:report ... WORKER <id> EXITED without reporting (dead)`. Optional:
a stall notice via `agent/status.rs check_idle` after a quiet window.
- Files: `service/pty_lifecycle.rs:196-234`, `service/hierarchy.rs:45`.
- Acceptance: integration test — spawn a child, kill its PTY without a report,
  assert a synthetic `-=-codeterm:report ... dead` lands in the parent's inject
  queue. Build + clippy + the new test.

### P4 — `--task-file` / stdin for `agent spawn` (kills the backtick zsh trap). HIGH/tiny
**Branch `orch-p4-task-file`.** Role: Rust CLI.

Daemon delivery is ALREADY backtick-safe (temp-file `task_file_expr`, byte-exact
round-trip test). The trap is the ORCHESTRATOR'S OWN shell evaluating backticks
in `--task` before argv reaches the CLI. `codeterm send` already has
`--from-file`/`--stdin`; give spawn the same.
- Change: add `--task-file <path>` (and `-` for stdin) → read → set the same
  `task` field (`args/agent.rs:129`, read in `cmd/agent.rs`). No daemon change.
- Acceptance: build + typecheck + a CLI test that `--task-file` with a body
  containing backticks/`$()` reaches the daemon byte-exact.

### P2 — Structured `agent report` (kills underspecified DONE/BLOCKED). MEDIUM
**Branch `orch-p2-structured-report`.** Role: Rust backend + CLI. (Do BEFORE P3.)

Today freeform: `AgentReport{id,from_pane_id,from_title,message,timestamp}`
(`reports.rs:8`); single positional CLI arg; in-memory capped `ReportStore`
(`reports.rs:17`, not persisted).
- Change: add optional `status(Done|Blocked|Partial)`, `branch`, `sha`,
  `changed_files`, `test_cmd`, `test_result`, `skipped`. CLI flags. Render a
  deterministic block into the injected text. Store structured so
  `agent reports --json` returns a typed list.
- Files: `agent/reports.rs:8`, `cli/src/args/agent.rs:54`,
  `socket/server/agent.rs:55`.
- Acceptance: report with all fields → `agent reports --json` returns them typed;
  inject text contains the deterministic block. Build + clippy + tests.

### P3 — Durable sqlite ledger (kills fragile in-memory state across restart/compact). MED-HIGH
**Branch `orch-p3-ledger`** (dep P2 — its schema feeds the table). Role: Rust backend.

`ReportStore` is in-memory (wiped on restart); hierarchy is only partially
persisted — `snapshot.rs:222-223` hardcodes `report_target_pane_id: None,
spawned_by_pane_id: None` on restore, and hidden ephemeral workers are excluded
→ after restart the orchestrator loses its worker map + reports.
- Change: per-orchestrator sqlite ledger (worker/branch/model/status/last-seen/
  gate result); `codeterm agent ledger --pane <id> [--json]`; back `ReportStore`
  with sqlite; persist `spawned_by` in snapshot (stop hardcoding None).
- Acceptance: spawn workers, report, restart daemon → ledger still lists them
  with last status; `spawned_by` survives restore. Build + clippy + tests.

### P5 — plugin sync/pump/is_agent_pane: DOC ONLY (not a code change)
Already smoothed + intentional. Captured as the dev fact below. Optional:
a `parity_test.rs` case asserting native vs JS agree on the `is_agent_pane` gate.

**Suggested order:** P1 + P4 first (independent, biggest win + tiny). Then
P2 → P3 (schema feeds ledger). P5 = doc hygiene.
**Leave alone (good engineering):** temp-file task delivery, the cron
death-fallback (P1 extends it), freeform inject as a transport (P2 layers on top).

---

## Skill rewrite (separate, parallel — independent of the code)

### Problem A — CUT dev leakage from the skill → 2 CodeTerm-dev mem facts (kind=fact, area=codeterm)
The 4 MECHANICS bullets are about BUILDING CodeTerm's plugin runtime, not
orchestrating in it (already in `docs/PLUGINS.md`). Extract, then delete from skill:
1. `fact_codeterm_plugin_runtime_sharp_edges` — single-threaded QuickJS VM;
   `call_json` holds the VM mutex for the whole call → block-for-a-turn work uses
   job-id/poll (`host.exec`→execStart/execPoll, worker.start/poll, agent.send/poll),
   never Promise-wrap (no event loop); send-time preprocessor stages gate on
   `is_agent_pane`/`is_user_turn_send` or shell panes get a `<context>` block and
   break; plugin-spawned agents use `PLUGIN_AGENT_TASK_PREAMBLE`, not the mem
   graph-worker preamble. Anchors: `prompts/plugin_agent_task_preamble.md`,
   `prompts.rs:39`, `host_compute.rs:7-26 & :471`, `vm.rs:72-80`,
   `chatpipeline/mod.rs:135`, `pane_transport_chat.rs:25`, commit `34a0ac44`.
2. `fact_codeterm_dev_push_hooks` — doc-only pushes may `git push --no-verify`
   (pre-push cargo check = known docs false-negative); `cargo fmt` is a pre-push
   hook (a worker that builds+tests but skips fmt passes its own gate, bounces on push).

### Problem B — general flaws to fix
1. Isolation contradicts the worker model → git WORKTREE per genuinely-independent
   track; single-checkout sequential ONLY for true data deps (A's SHA → B's base).
2. Orchestrator-owned gate is a bottleneck + non-hermetic → ≥1 gate in a CLEAN
   checkout; per-merge smoke check, not one giant e2e at the end.
3. Strict TDD as universal law is cargo-cult + unverifiable → risk-based acceptance
   per track (logic=tests, refactor=build+typecheck, schema=migration dry-run,
   UI=screenshot/e2e, docs=link-check). TDD is one option.
4. Heartbeat self-send is polling dressed up + no durable ledger → **DEMOTE, do
   NOT delete** (correction to Sagan's "kill it"). P1's watchdog replaces it ONLY
   as the worker-DEATH signal, and P3's ledger replaces its STATE role. But a
   SPARSE long-interval `codeterm send --delay` self-send still earns its place as
   the backstop the watchdog cannot cover: (a) the orchestrator's OWN liveness —
   re-wakes a stalled/compacted orchestrator session (watchdog can't wake a dead
   orchestrator); (b) a HUNG-but-alive worker — PTY never died so the watchdog
   never fires, but "no progress in N min" still needs catching; (c) external
   polling the daemon can't push (CI, remote state). So: watchdog = primary
   worker-death wake; ledger = durable state; sparse `--delay` = liveness backstop.
   Proven live 2026-06-19: a heartbeat re-woke the orchestrator as a backstop after
   a track had already merged — exactly the self-liveness case the watchdog misses.
5. Branch hygiene reactive (`--no-merged` misses unpushed/dirty/squash/dup) →
   ledger is the source of truth (proactive).
6. Worker report underspecified → enforce schema (P2): status/branch/sha/
   changed-files/test-cmd+result/skipped/plan-deviations.
7. No rebase/conflict protocol for bounced red branches → require fetch+rebase
   onto integration head, re-gate, re-report SHA; never merge base != head.
8. No security/destructive boundaries → declare off-limits zones per track
   (secrets/.env, migrations, package scripts, CI, install paths, signing);
   touching one = mandatory second review + human OK.
9. Missing tiering → LITE/HEAVY as the FIRST decision (ceremony scales to blast
   radius); model-scaling (cheap model for mechanical tracks, Opus for design/
   ambiguous/security).

### Concrete worker-spawn recipe to LOCK INTO the rewrite (section 3+4)
The current skill's spawn line (`--task '...'` inline + "no backticks" warning +
"workspace per repo NOT worktrees") is the OLD pattern. The rewrite must replace
it with the post-P1/P4 recipe battle-tested on 2026-06-19:

1. **Integration branch** off the base (`git branch <integration> <base>`, push).
   **Worktree per INDEPENDENT track** (`git worktree add -b <track> <path> <integration>`),
   placed OUTSIDE the repo dir (e.g. `/Users/kandrey/Developer/wk-<track>`).
   Single-checkout sequential ONLY for true data deps (A's SHA → B's base).
2. Fresh worktree needs build stubs for cargo/tauri: `node scripts/prepare-embed-stub.js`
   + `node scripts/prepare-cli-stub.js` + a `dist/index.html` stub. Gitignored,
   NEVER committed. **NEVER run those scripts in the MAIN checkout** — they make
   circular self-symlinks there (ELOOP breaks the pre-push hook). Safe in a worktree.
3. Shared `CARGO_TARGET_DIR=/Users/kandrey/Developer/.psetup-target` across
   worktrees (parallel cargo without lock contention).
4. Spawn (post-P4): `codeterm agent spawn claude --model opus --role Worker
   --parent $CODETERM_PANE_ID --title T --cwd <worktree> --task-file <path>`.
   PRE-P4 fallback: `--task "$(cat <path>)"` — command substitution inside double
   quotes passes backticks/`$()` literally; NEVER an inline backticked `--task`.
5. Task file = self-contained English: the change + exact file anchors; the
   build-stub setup; the GATE (fmt + build + targeted tests + pre-push hook clippy
   `--all-targets` + the `.githooks/pre-push` CLIPPY_ALLOW `-A` flags for src-tauri
   AND cli; `manual_range_contains`/`assertions_on_constants` are NOT allowed);
   "commit before reporting, DO NOT push"; report via structured `agent report`
   (P2 flags); "Worker NEVER spawns workers — report Blocked if decomposition needed."
6. Gate-merge-yourself: `git checkout <integration>` in main, `git merge --no-ff
   <track>`, re-run the track's targeted tests, push (pre-push hook = authoritative
   gate). Then `git worktree remove <path>`; assert no stranded branches.

### New skill structure (provider-agnostic)
1 pick tier · 2 model scaling · 3 worktree isolation · 4 worker contract +
structured report + risk-based acceptance · 5 liveness & durable ledger —
react-to-report is PRIMARY (native worker-death watchdog injects on PTY death),
durable ledger holds state across restart/compaction, and a SPARSE long-interval
`--delay` self-send stays as the liveness backstop (orchestrator self-wake on
stall/compaction, hung-but-alive worker, external poll) — demote the heartbeat,
don't delete it · 6 integration gate (clean-checkout + per-merge smoke) ·
7 rebase protocol · 8 protected areas · 9 slimmed provider-agnostic mechanics
(no plugin-runtime / Rust build notes — those went to the dev facts).

**Reference for the skill rewrite (fetch when doing Problem A/B, NOT before):**
the rollacode orchestrator's full proposal + both reviewers' per-flaw notes
(Reviewer A / Opus = 9 cost+correctness items; Reviewer B / Codex = 12
adversarial items; + consolidation table of agreements vs unique catches + the
2 earned invariants to keep) is in a mesh paste:
`codeterm paste get --mesh Mac-Air 4y6v8c` (24h TTL from 2026-06-19 ~morning —
re-request from the Mac-Air orchestrator if expired). Fold its nuance into the
B-flaw fixes + the new structure.

Note the deliciously recursive bit: this skill upgrade is GATED on the code
changes landing (the skill can only drop the heartbeat once P1 ships, the manual
ledger once P3 ships). So: code first (P1/P4 → P2/P3), then rewrite the skill to
match the new reality, then move the 2 dev facts into mem.

<!-- revision: 1 -->
