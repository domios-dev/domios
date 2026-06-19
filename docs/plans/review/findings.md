# Phase 2 — accumulated review findings (triage when all passes report)

## Pass 2 — providers + bitwarden (codex) — DONE
HIGH:
- cursor plugin.ts:80 — trustPrefix() ALWAYS prewrites .workspace-trusted even when skipPermissions=false -> silently bypasses Cursor trust gate. Fix: gate prewrite behind skipPermissions. (NOTE: this is Andrey's intentional incantation for --force; refine, don't remove.)
- bitwarden plugin.ts:197/270 — persists MASTER PASSWORD (tryUnlockFromStore + stores K_MASTER) -> silent re-unlock w/ long-lived master pw copy. SECURITY. Fix: store BW_SESSION only, no master pw by default; opt-in + TTL.
MED:
- bitwarden plugin.ts:430 — secretInit() shells `bw status` (30s timeout) + may auto-unlock on load -> blocking. Fix: cheap non-mutating init, async job-id/poll, unlock only on explicit action.
- bitwarden ui/main.tsx:56 — iframe probes status+serverUrl on mount (bw status host.exec) = THE "window expand on load" smell. Fix: render from cached, defer probes, stable panel size.
- bitwarden plugin.ts:477 — setServerUrl stores any URL; `bw config server <url>` reaches arbitrary endpoints beyond manifest network scope. Fix: validate vs declared perms / prompt.
- opencode plugin.ts:281/291 — detectSessionModel falls back to "latest model in cwd" -> VIOLATES pane-own-session rule (#3). Fix: return null on session miss, no cwd-latest guess.
- git plugin.ts:72 — statusBubble shells out 2x synchronously per footer bubble -> hot-path block. Fix: cache per-cwd, refresh by watcher.
- git plugin.ts:525 — viewCall trusts iframe-supplied cwd for mutating git (add/commit/push). Fix: host-stamp cwd from pane, ignore iframe cwd for mutations.
LOW:
- opencode plugin.ts:502 — session_start post ends .catch(()=>{}) (swallows hook delivery failures). Fix: log on debug.
- claude:1112 / codex:536 — duplicated menu parse/nav logic. Fix: shared helper (parity-guarded).
- git plugin.ts:556 — viewCall wraps errors as String(e), loses method/cmd context.

CLEAN: lmstudio uses host.toolcall.parse (ok/malformed/none) + execStart/poll; launch args shell-quoted; claude/codex log first invalid JSONL; bitwarden uses argv/env/stdin for secrets.

## ROG cross-platform findings
- F6 (HIGH, cross-platform RUNTIME bug) — AS3/AS4 (claude) + AS2 (codex) pane-state classifiers return unknown/low-confidence on Windows: fixtures + real Windows pane output have CRLF; classifier matches on \n boundaries so \r breaks structural patterns -> conf 0.5499/0.6499, nav actions undefined. Fix: normalize \r\n and \r -> \n at the START of classifyPaneState (robust for real Windows output) + add .gitattributes keeping tests/fixtures/pane-state/*.txt as LF for determinism. macOS passes (LF), Windows fails -> confirmed platform.
- F5 (Windows test-harness gotcha) — `cargo test` of the app crate fails on Windows while a dev build runs from the same target dir (tauri-build can't overwrite the running exe, Os code 5). Workaround: CARGO_TARGET_DIR=target-test. Doc this in TESTING.md.

## Pass 1b — capability discipline (codex) — DONE
- MED opencode plugin.ts:281 detectSessionModel cwd-latest fallback (rule#3) [dup P2]
- MED src-tauri/.../provider.rs:~530 generic JsProvider detect_session_id returns newest-for-cwd (no detectSessionId) -> mirror Claude guard (None if >1 candidate)
- MED provider-cursor plugin.ts:124 detectSessionId newest-under-cwd-hash -> refuse if >1 candidate
- MED parity_test.rs parity NOT in lockstep: missing cases for buildLaunchCommand (codex/opencode/cursor), detectSessionId, sessionExists, sessionCreatedMs, detectActiveSession, detectSessionModel, sessionFilePath (codex/cursor), enumerateSessions, usesStructuredChat/readStructuredChat (opencode/cursor), parseSessionDelta/parseSessionMessages (codex/cursor) (rule#2)
- LOW host_compute.rs:82 host.worker/agent default backend hardcodes claude
- LOW src/store/multiplexer/index.tsx:387 spawnClaudeTab + CommandPalette:510 "Spawn Claude" hardcoded; MemoryPanel AGENTS=[claude]
- CLEAN: core registry/spawn/chat dispatch by capability; paneProvider/chatUiMode capability-backed; version lockstep 1.4.6; host surface properly permission-gated

## Pass 3 — core seams (opus) — DONE
- MED pane_transport_chat.rs:181 chatbackend_reopen_if_restored RACE: no once-guard, 3 entry points -> double poll loop + 2 sessions. Fix: per-pane Once/CAS on restored_from_snapshot.
- MED closed/mod.rs:255/300/321 reopen TAKES (deletes) entry before materialize -> mid-loop failure = irreversible partial restore. Fix: materialize-then-delete / staging.
- MED closed/mod.rs:239 materialize sets chat_view only from agent_type -> chatBackend reopen gets chat_view=FALSE (spawn sets TRUE). Fix: chat_view=true when backing=ChatBackend.
- LOW cascade reopen activates a leaf not the orchestrator (closed_at DESC sort); collect_subtree unbounded recursion (cycle guard); messenger mux.write() for read-only; inbound emitted before send (no rollback); dangling tab on launch Err; repair_json brace-balance ignores string literals.
- CLEAN: reactor lock discipline, auth-guard, dedup, poll-loop planner, history single-writer, messenger markers, toolcall tri-state, closed-store parameterized SQL.

## Pass 4 — async/locks/size (sonnet) — file-size mostly clean (cli/cmd/plugin.rs SPLIT); 1 lock-ordering FINDING (re-fetch pending).

## Pass 4 (sonnet) — captured so far
- MED hot-path src/components/Extensions/ExtensionsView.tsx:102-104 — every Extensions-panel visit fires plugin_manager_action{kind:refresh-channel} (network). Fix: cache + refresh on demand/interval, not every mount.
- file-size: CLEAN (cli/src/cmd/plugin.rs was SPLIT; no allowlist growth).
- lock-ordering FINDING 1: pending re-report (P3-opus independently audited locks -> CLEAN; treat P4's as to-verify).

## Pass 4 (sonnet) — FULL report
HIGH async/spawn_blocking:
- glance.rs:32,42 + bubble.rs:31 + view.rs:31 — async fns call_json (QuickJS Mutex+JS) WITHOUT spawn_blocking. Fix: wrap in Tauri command path.
- web_routes.rs:620 plugin_snapshot (3x subprocess + git clone) inline; :632 plugin_action (Command CLI) inline; :829 plugin_ui std::fs::read on runtime. Fix: spawn_blocking / tokio::fs (Tauri path wraps, web-mirror doesn't).
HIGH arch:
- cli.rs:8 run_plugin_cli — daemon forks its OWN codeterm CLI for plugin ops (violates "daemon never shells its own CLI"). Fix: in-process shared crate. (DEFER: bigger refactor; spawn_blocking mitigates freeze meanwhile.)
HIGH lock:
- hot_reload.rs:269-284 tick() holds state.lock() across loop while acquiring vms/pending_reload/activated_startup + calling unregister_provider/deregister_plugin -> inversion vs full_load + reentrancy deadlock risk (author knew, comment :344). Fix: collect IDs under short state lock, drop, then unregister.
HIGH file-size (F7, CI-block):
- provider-claude/src/plugin.ts = 1264 lines (NOT allowlisted) -> check-file-sizes --fail blocks CI. Split (classify.ts/session.ts/models.ts), rebuild bundle, parity green.
MED:
- model_catalog.rs:176 block_on inside spawn_blocking (nested deadlock). 
- pty_lifecycle.rs:213-215 cron_state TOCTOU double-lock.
- ExtensionsView.tsx:102-104 refresh-channel on every panel visit, no FE TTL (5 forks). Fix: FE TTL vs backend last_refresh.
LOW:
- state_reactor.rs:100-102 agent_tracker locked N+1/tick. host_api.rs:744-749 dead code (HOST_GLOBAL, _link_path_buf_unused). dispatch/mod.rs 865>800; hot_reload/provider/registry .rs within 14-19 of 1000 (split proactively).
CLEAN: cli/cmd/plugin.rs split done (remove from allowlist).

## ROG Playwright smoke (18/10) triage
- 5x lmstudio.spec = ENV (lmstudio not installed on ROG dev; green on macOS). PW-robustness item: skip-when-provider-absent guard. NOT a bug.
- 3x settings-theme-select not visible = NOT a branch regression (testid exists SettingsPanel.tsx:252; src/components/Settings unchanged vs master). Likely WebView2 visibility-timing or pre-existing. ROG verifying vs prod + DOM-absent-vs-not-visible.
- 2x extensions page-closed = cascade after settings; no backend panic. Re-check after settings.

## Settings PW triage — RESOLVED (not a branch bug)
- Smoke error = "element(s) not found" (DOM-absent, zero nodes), not "hidden" -> the Settings panel/tab never opened in time = Windows/WebView2 navigation-timing flake. Source unchanged (SettingsPanel.tsx:252). ROG getting a definitive DOM/ARIA trace from the full suite. Prod PW comparison declined (would require bouncing prod 7685 w/ CDP — prohibited).

## ROG Playwright golden (38/14/1flaky/1skip on 41c8e57d = PRE wave-1/2) — triage
- live-agent cluster (5) incl cross-pane-chat 400 "chat snapshot failed": ENV/harness — pane_chat path untouched by campaign (f0b8abf6 pre-campaign); 400 = no session to snapshot, downstream of headless PW lacking LLM auth + CODETERM_TEST_CLOCK timing. P0.2 fixed+manually verified. NOT a regression.
- getUserMedia voice (2): ENV (no mic in WebView2 test).
- network-detection web-access (2): possible Windows addr-selection diff but pre-existing (no campaign code touches it); ROG checking vs prod.
- UI-not-found (5): untouched flows; pre-existing/harness; extensions improves post-wave3 (TTL+force-refresh).
- DEFINITIVE: ROG re-runs on FINAL tip after wave-3. macOS desktop golden N/A (PW = Windows/WebView2 only).
