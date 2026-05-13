# Full Project Code Review — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce a comprehensive, ranked findings report for every source file in the CodeTerm project covering correctness, architecture, and consistency.

**Architecture:** 12 parallel domain agents each read all files in their subsystem and write a structured findings report; one aggregator agent then reads all 12 reports and produces a single consolidated document.

**Tech Stack:** Rust (src-tauri, cli, crates), TypeScript/TSX (src/), Read tool per file, Write tool for output reports.

**Output directory:** `docs/superpowers/review/2026-05-13/`

**Severity levels:** CRITICAL (panic/deadlock/data-loss/security) → HIGH (incorrect behavior, broken abstraction, allowlist growth) → MEDIUM (DRY violation, naming drift, responsibility creep) → LOW (style nit, advisory)

**Lock ordering (from CLAUDE.md):** mux → output_buf → screen_state → agent_tracker → pty. Any inversion = CRITICAL.

---

## Per-Agent Output Format

Each domain agent writes its report to `docs/superpowers/review/2026-05-13/domain-<N>-<name>.md` using this structure:

```markdown
# Domain Review: <Name>

## Files Reviewed
- list every file read

## Findings

### <relative/path/to/file.rs>

#### CRITICAL
- `file.rs:42` — bare `unwrap()` on network I/O result; panics if connection drops

#### HIGH
- `file.rs:120` — lock acquired inside another lock without following mux→pty ordering

#### MEDIUM
- `file.rs:88` — same URL-parsing logic duplicated from `other_file.rs:55`

#### LOW
- `file.rs:200` — variable named `tmp` — rename to describe its purpose

#### OK (if nothing found)

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | N |
| HIGH | N |
| MEDIUM | N |
| LOW | N |
| Total | N |
```

---

## Task 1: Domain Agent 1 — Agent & Plugin

**Files reviewed:**
- `src-tauri/src/agent/` (all .rs files)
- `src-tauri/src/plugin/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-01-agent-plugin.md`

- [ ] **Step 1: Read every file in `src-tauri/src/agent/`**

Files to read in order:
`agent/mod.rs`, `agent/detector.rs`, `agent/git_info.rs`, `agent/model_catalog.rs`, `agent/models.rs`, `agent/prompt.rs`, `agent/provider.rs`, `agent/registry.rs`, `agent/reports.rs`, `agent/run_watcher.rs`, `agent/status.rs`, `agent/usage_parser.rs`

- [ ] **Step 2: Read every file in `src-tauri/src/plugin/`**

Files to read in order:
`plugin/mod.rs`, `plugin/agent_claude/provider/mod.rs` (and sub-files), `plugin/agent_opencode/provider.rs` (and any sub-files)

Run: `find src-tauri/src/plugin -name "*.rs" | sort` to discover exact file list.

- [ ] **Step 3: Write findings report**

For each file: check correctness (unwrap/expect in non-test paths, error propagation, panics), architecture (file size vs 1000-line limit, mod.rs vs 300-line limit, SRP), consistency (naming conventions, DRY, dead code, TODO/FIXME in prod).

Write to `docs/superpowers/review/2026-05-13/domain-01-agent-plugin.md`.

---

## Task 2: Domain Agent 2 — API Core Routes

**Files reviewed:**
- `src-tauri/src/api/` excluding `mesh_routes/` and `mem_routes/` subdirs

**Output:** `docs/superpowers/review/2026-05-13/domain-02-api-core.md`

- [ ] **Step 1: Discover and read all API core route files**

Run: `find src-tauri/src/api -maxdepth 1 -name "*.rs" | sort`

Expect: `agent_routes.rs`, `auth.rs`, `bridge_routes.rs`, `config_routes.rs`, `cron_routes.rs`, `events.rs`, `health_routes.rs`, `hook_routes.rs`, `logs_routes.rs`, `mod.rs`, `network_routes.rs`, `notification_routes.rs`, `os_notify.rs`, `pane_routes.rs`, `routes.rs`, `services_routes.rs`, `session_routes.rs`, `theme_routes.rs`, `updater_routes.rs`, `usage_routes.rs`, `webhook_routes.rs`, `workspace_routes.rs`

- [ ] **Step 2: Read each file end-to-end**

Pay special attention to `pane_routes.rs` (722 lines — near limit).

- [ ] **Step 3: Write findings report**

Check: consistent HTTP status codes across routes, consistent error response shapes, auth middleware applied consistently, no business logic leaking into route handlers, handler naming conventions, proper use of `?` vs `unwrap`, missing input validation at boundary.

Write to `docs/superpowers/review/2026-05-13/domain-02-api-core.md`.

---

## Task 3: Domain Agent 3 — Memory Subsystem

**Files reviewed:**
- `src-tauri/src/mem/` (all .rs files)
- `src-tauri/src/api/mem_routes/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-03-memory.md`

- [ ] **Step 1: Read all mem/ files**

Run: `find src-tauri/src/mem -name "*.rs" | sort`

Pay special attention to: `mem/dreaming.rs` (1095 lines — only allowlisted file, must not grow), `mem/search.rs` (777 lines), `mem/graph.rs` (795 lines), `mem/rerank.rs` (760 lines).

- [ ] **Step 2: Read all api/mem_routes/ files**

Run: `find src-tauri/src/api/mem_routes -name "*.rs" | sort`

Pay special attention to: `mem_routes/items.rs` (788 lines), `mem_routes/calibration.rs` (640 lines).

- [ ] **Step 3: Write findings report**

Check: embedding correctness, search ranking logic consistency, worker spawn safety, dreaming.rs growth (must shrink not grow), lock acquisition patterns in concurrent mem ops, SQL injection risk in dynamic queries, consistency between mem store API and CLI interface.

Write to `docs/superpowers/review/2026-05-13/domain-03-memory.md`.

---

## Task 4: Domain Agent 4 — Mesh Subsystem

**Files reviewed:**
- `src-tauri/src/mesh/` (all .rs files)
- `src-tauri/src/api/mesh_routes/` (all .rs files)
- `src-tauri/src/commands_mesh.rs`

**Output:** `docs/superpowers/review/2026-05-13/domain-04-mesh.md`

- [ ] **Step 1: Read all mesh/ files**

Run: `find src-tauri/src/mesh -name "*.rs" | sort`

Pay special attention to: `mesh/mod.rs` (768 lines — exceeds 300-line mod.rs advisory), `mesh/discovery.rs` (699 lines).

- [ ] **Step 2: Read all api/mesh_routes/ files**

Run: `find src-tauri/src/api/mesh_routes -name "*.rs" | sort`

Files: `announce.rs`, `connection.rs`, `introduction.rs`, `mod.rs`, `pairing.rs`, `relay.rs`

- [ ] **Step 3: Read `src-tauri/src/commands_mesh.rs`**

- [ ] **Step 4: Write findings report**

Check: peer authentication gaps (known security audit findings in mem), replay attack surface, MITM risk in pairing flow, gossip broadcast correctness (3rd-peer bug documented in mem), lock ordering in concurrent peer state, missing bounds on gossip fan-out.

Write to `docs/superpowers/review/2026-05-13/domain-04-mesh.md`.

---

## Task 5: Domain Agent 5 — Bridge & Socket

**Files reviewed:**
- `src-tauri/src/bridge/` (all .rs files)
- `src-tauri/src/socket/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-05-bridge-socket.md`

- [ ] **Step 1: Read all bridge/ files**

Run: `find src-tauri/src/bridge -name "*.rs" | sort`

Files include: `inbound_buffer.rs`, `manager/mod.rs` (610 lines — exceeds advisory), `manager/tests.rs`, `migrate_test.rs`, `mod.rs`, `response.rs`, `sanitize.rs`, `supervisor.rs`, `telegram.rs`, `text_hooks.rs`, `traits.rs`, `voice_transcribe.rs`

- [ ] **Step 2: Read all socket/ files**

Run: `find src-tauri/src/socket -name "*.rs" | sort`

Files include: `socket/mod.rs`, `server/agent.rs`, `server/bridge.rs`, `server/cron.rs`, `server/mod.rs` (381 lines — exceeds advisory), `server/notification.rs`, `server/pane.rs`, `server/services.rs`, `server/workspace.rs`

- [ ] **Step 3: Write findings report**

Check: bridge supervisor restart safety, sanitize.rs correctness (injection/XSS prevention), Telegram token handling security, voice transcribe error paths, socket server connection lifecycle (cleanup on drop), inbound buffer overflow protection.

Write to `docs/superpowers/review/2026-05-13/domain-05-bridge-socket.md`.

---

## Task 6: Domain Agent 6 — Orchestration Core

**Files reviewed:**
- `src-tauri/src/multiplexer/` (all .rs files)
- `src-tauri/src/service/` (all .rs files)
- `src-tauri/src/dispatch/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-06-orchestration.md`

- [ ] **Step 1: Read all multiplexer/ files**

Run: `find src-tauri/src/multiplexer -name "*.rs" | sort`

Pay attention to `multiplexer/state/tests.rs` (938 lines).

- [ ] **Step 2: Read all service/ files**

Run: `find src-tauri/src/service -name "*.rs" | sort`

Pay attention to: `service/orchestrator.rs` (835 lines — near limit), `service/spawn.rs` (707 lines).

- [ ] **Step 3: Read all dispatch/ files**

Run: `find src-tauri/src/dispatch -name "*.rs" | sort`

Note: `dispatch/mod.rs` is 609 lines — well over the 300-line facade limit.

- [ ] **Step 4: Write findings report**

Check: lock ordering compliance (mux → output_buf → screen_state → agent_tracker → pty — CLAUDE.md mandated), orchestrator.rs responsibility count (835 lines suggests multiple concerns), PTY lifecycle cleanup on crash, spawn error propagation, dispatch/mod.rs logic that should be in concrete files.

Write to `docs/superpowers/review/2026-05-13/domain-06-orchestration.md`.

---

## Task 7: Domain Agent 7 — Config & Secrets

**Files reviewed:**
- `src-tauri/src/config/` (all .rs files)
- `src-tauri/src/secrets/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-07-config-secrets.md`

- [ ] **Step 1: Read all config/ files**

Run: `find src-tauri/src/config -name "*.rs" | sort`

Pay attention to: `config/schema/mod.rs` (751 lines — exceeds mod.rs advisory), `config/mod.rs` (476 lines — exceeds advisory).

- [ ] **Step 2: Read all secrets/ files**

Run: `find src-tauri/src/secrets -name "*.rs" | sort`

Pay attention to: `secrets/mod.rs` (563 lines), `secrets/local.rs` (715 lines), `secrets/bitwarden/tests.rs` (655 lines).

- [ ] **Step 3: Write findings report**

Check: secrets never logged (search for `log::`, `tracing::`, `println!` near secret values), config validation at boundaries (missing `.validate()` calls), schema/mod.rs logic leaking into facade, bitwarden integration error paths (network timeout handling), migration safety for config schema changes.

Write to `docs/superpowers/review/2026-05-13/domain-07-config-secrets.md`.

---

## Task 8: Domain Agent 8 — Supporting Services

**Files reviewed:**
- `src-tauri/src/services/` (all .rs files)
- `src-tauri/src/session/` (all .rs files)
- `src-tauri/src/update/` (all .rs files)
- `src-tauri/src/usage/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-08-supporting-services.md`

- [ ] **Step 1: Read all services/ files**

Run: `find src-tauri/src/services -name "*.rs" | sort`

Files: `activity.rs`, `health/http.rs`, `health/mod.rs`, `health/process.rs`, `health/scheduler.rs`, `health/script.rs`, `health/tcp.rs`, `mod.rs`, `push.rs`, `smart_add.rs`, `store/cron_view.rs`, `store/crud.rs`, `store/mod.rs`, `store/schema.rs`, `store/tests.rs`, `store/types.rs`, `tokens.rs`

- [ ] **Step 2: Read all session/, update/, usage/ files**

Run: `find src-tauri/src/session src-tauri/src/update src-tauri/src/usage -name "*.rs" | sort`

Pay attention to: `usage/projection.rs` (769 lines), `usage/projection_tests.rs`.

- [ ] **Step 3: Write findings report**

Check: health check scheduler memory-safety (no Arc cycles), update apply atomicity (partial write on crash), usage projection correctness (billing implications of over/under counting), push notification token expiry handling, session store migration safety, cron store consistency.

Write to `docs/superpowers/review/2026-05-13/domain-08-supporting-services.md`.

---

## Task 9: Domain Agent 9 — Startup & Commands

**Files reviewed:**
- `src-tauri/src/startup/` (all .rs files)
- `src-tauri/src/commands/` (all .rs files)
- Top-level `src-tauri/src/*.rs` (excluding main.rs and lib.rs which are entrypoints)

**Output:** `docs/superpowers/review/2026-05-13/domain-09-startup-commands.md`

- [ ] **Step 1: Read all startup/ files**

Run: `find src-tauri/src/startup -name "*.rs" | sort`

Files: `background.rs`, `bridge.rs`, `cli_install.rs`, `cli_shim.rs`, `logging.rs`, `mod.rs`, `servers.rs`, `states.rs`, `window.rs`

- [ ] **Step 2: Read all commands/ files**

Run: `find src-tauri/src/commands -name "*.rs" | sort`

Files: `agent.rs`, `clipboard.rs`, `completions.rs`, `cron.rs`, `hooks.rs`, `mod.rs`, `pane.rs`, `pty.rs`, `services.rs`, `skills.rs`, `system.rs`, `system_info.rs`

- [ ] **Step 3: Read top-level src-tauri/src/*.rs files**

Run: `find src-tauri/src -maxdepth 1 -name "*.rs" | sort`

Files include: `attachments.rs`, `autostart.rs`, `clipboard_files.rs`, `commands_mesh.rs`, `shell.rs`, and any others at top level.

- [ ] **Step 4: Write findings report**

Check: startup ordering (are background services started before they're needed?), Tauri command error handling (all commands should return `Result<_, String>` not panic), CLI shim correctness, logging setup race conditions, window creation on non-main-thread risk.

Write to `docs/superpowers/review/2026-05-13/domain-09-startup-commands.md`.

---

## Task 10: Domain Agent 10 — CLI

**Files reviewed:**
- `cli/src/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-10-cli.md`

- [ ] **Step 1: Discover all CLI files**

Run: `find cli/src -name "*.rs" | sort`

Pay attention to: `cli/src/main.rs` (703 lines), `cli/src/client.rs` (file likely large), `cli/src/cmd/worker.rs` (744 lines), `cli/src/cmd/mem/mod.rs` (713 lines), `cli/src/args/mem.rs` (701 lines), `cli/src/cmd/bridge/migrate.rs` (668 lines).

- [ ] **Step 2: Read every file in cli/src/**

Start with `main.rs`, then `args/`, then `cmd/`, then `mesh_cli/`.

- [ ] **Step 3: Write findings report**

Check: command error handling (no panics in user-facing paths), argument validation before API calls, proper exit codes, API client error messages (user-facing clarity), `cmd/mem/mod.rs` at 713 lines — mod.rs should be facade only, `cmd/worker.rs` at 744 lines — responsibility check, `cmd/bridge/migrate.rs` — migration safety and rollback handling.

Write to `docs/superpowers/review/2026-05-13/domain-10-cli.md`.

---

## Task 11: Domain Agent 11 — mem-store Crate

**Files reviewed:**
- `crates/mem-store/src/` (all .rs files)

**Output:** `docs/superpowers/review/2026-05-13/domain-11-mem-store-crate.md`

- [ ] **Step 1: Read all crates/mem-store/src/ files**

Run: `find crates/mem-store/src -name "*.rs" | sort`

Files: `audit.rs`, `cluster.rs`, `error.rs`, `graph.rs` (416 lines), `lesson.rs`, `lib.rs`, `migrations.rs`, `plan_format.rs` (271 lines), `tests/mod.rs` (653 lines), `tests/cluster.rs`, `tests/lesson.rs`

- [ ] **Step 2: Write findings report**

Check: public API surface (is `lib.rs` re-exporting the right things?), `graph.rs` correctness (graph operations should be transactional), migration idempotency, error type completeness (`error.rs` has all needed variants?), `tests/mod.rs` at 653 lines — likely has logic that should be in dedicated files, plan_format.rs format correctness.

Write to `docs/superpowers/review/2026-05-13/domain-11-mem-store-crate.md`.

---

## Task 12: Domain Agent 12 — Frontend

**Files reviewed:**
- `src/` (all .ts and .tsx files, excluding node_modules and dist)

**Output:** `docs/superpowers/review/2026-05-13/domain-12-frontend.md`

- [ ] **Step 1: Discover all frontend files**

Run: `find src -name "*.ts" -o -name "*.tsx" | sort`

Pay attention to large files: `Settings/SecretsTab.tsx` (781 lines), `App.tsx` (683 lines), `Sidebar/Sidebar.tsx` (663 lines), `Onboarding/Onboarding.tsx` (657 lines), `ChatView/ChatView.tsx` (602 lines), `ChatView/MessageBubble.tsx` (581 lines), `store/multiplexer/index.tsx` (543 lines).

- [ ] **Step 2: Read all files in src/**

Read in order: `main.tsx`, `App.tsx`, `lib/types.ts`, `store/`, `hooks/`, `components/` (alphabetical by component).

- [ ] **Step 3: Write findings report**

Check: `any` types in production code (type safety gaps), unhandled promise rejections, missing `key` props in lists, stale closure bugs in hooks, SecretsTab.tsx at 781 lines (should be split), App.tsx at 683 lines (too much in root), missing cleanup in `useEffect` returns, `console.log` left in production code, inconsistent error handling patterns (some use toast, some silent catch), multiplexer reducer correctness (check for mutation of state).

Write to `docs/superpowers/review/2026-05-13/domain-12-frontend.md`.

---

## Task 13: Aggregator — Consolidated Report

**Depends on:** Tasks 1–12 all complete (all 12 domain reports written)

**Output:** `docs/superpowers/review/2026-05-13/consolidated.md`

- [ ] **Step 1: Read all 12 domain reports**

Read in order:
`docs/superpowers/review/2026-05-13/domain-01-agent-plugin.md`
`docs/superpowers/review/2026-05-13/domain-02-api-core.md`
`docs/superpowers/review/2026-05-13/domain-03-memory.md`
`docs/superpowers/review/2026-05-13/domain-04-mesh.md`
`docs/superpowers/review/2026-05-13/domain-05-bridge-socket.md`
`docs/superpowers/review/2026-05-13/domain-06-orchestration.md`
`docs/superpowers/review/2026-05-13/domain-07-config-secrets.md`
`docs/superpowers/review/2026-05-13/domain-08-supporting-services.md`
`docs/superpowers/review/2026-05-13/domain-09-startup-commands.md`
`docs/superpowers/review/2026-05-13/domain-10-cli.md`
`docs/superpowers/review/2026-05-13/domain-11-mem-store-crate.md`
`docs/superpowers/review/2026-05-13/domain-12-frontend.md`

- [ ] **Step 2: Write consolidated report**

Structure of `docs/superpowers/review/2026-05-13/consolidated.md`:

```markdown
# CodeTerm Full Code Review — Consolidated Report
Date: 2026-05-13

## Executive Summary

Total findings:
| Severity | Count | Domains affected |
|----------|-------|-----------------|
| CRITICAL | N | list |
| HIGH | N | list |
| MEDIUM | N | list |
| LOW | N | list |

## Cross-Cutting Patterns

(Patterns appearing in 3+ domains — name each pattern, list affected files)

## Per-Domain Summary

### Domain 1: Agent & Plugin
One paragraph: top issues, overall health.

(repeat for each domain)

## All CRITICAL Findings

(Every CRITICAL item from all domains, with file:line and description)

## All HIGH Findings

(Every HIGH item from all domains, with file:line and description)

## Systemic Recommendations

3-5 concrete structural changes that would address the most findings.
```

---

## Self-Review Checklist

- [ ] Every domain task specifies exact file discovery commands
- [ ] Every domain task specifies the exact output path
- [ ] Per-agent output format is consistent (same severity headings)
- [ ] Aggregator reads all 12 reports before writing (sequential dependency documented)
- [ ] No placeholders ("TBD", "etc.") in any task
- [ ] Output directory created before agents write to it
