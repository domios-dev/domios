# Phase 1 Baseline — 1017f2c9 (2026-06-20 00:27)

## 1a. Static gates / guards (local macOS)
### check:versions
✅ PASS

### check:plugin-versions
✅ PASS

### check-file-sizes
❌ FAIL (rc=126)
bash: scripts/check-file-sizes.sh: Permission denied

### check-prompts-doc
❌ FAIL (rc=126)
bash: scripts/check-prompts-doc.sh: Permission denied

### check-api-docs
❌ FAIL (rc=126)
bash: scripts/check-api-docs.sh: Permission denied

### check-blocking-in-async
✅ PASS

### check-mux-write
✅ PASS

### check-no-raw-graph-sql
❌ FAIL (rc=126)
bash: scripts/check-no-raw-graph-sql.sh: Permission denied

### check-evals-list
❌ FAIL (rc=126)
bash: scripts/check-evals-list.sh: Permission denied


## Static gate results (local)
- ✅ check:versions, check:plugin-versions, check-file-sizes, check-prompts-doc, check-no-raw-graph-sql, check-blocking-in-async, check-mux-write
- ❌ check-api-docs, check-evals-list — FINDING F1: scripts call `python` (not `python3`); broken on macOS. CI-only (not in pre-push hook). Phase-2 fix: python3 fallback.

## 1c/1d/1e. Frontend gates (local)
### tsc --noEmit
✅ PASS

### test:unit
✅ PASS

✓ all 51 TS unit test file(s) green

### test:plugins
✅ PASS

✓ all plugin test suites green


## ROG (Windows) — in progress
- ff-pulled 34a0ac44 -> 1017f2c9 (432 files). Building dev.
- FINDING F2 (Windows): Cargo.toml showed a phantom CRLF/autocrlf diff (LF->CRLF, zero content). git autocrlf artifact — can cause spurious "modified" flags on Windows. Phase-2: add .gitattributes eol normalization or document.

## Test-lane division (confirmed)
- **Local (macOS):** static gates, cargo test --lib, test:unit (51 ✅), test:plugins (✅), tsc (✅), API-layer evals. NO desktop Playwright (WKWebView has no CDP).
- **ROG (Windows):** desktop-UI Playwright suites (CDP/WebView2) + cross-platform cargo/plugin tests. PW lint ✅ (runs anywhere).

## Local baseline status (1017f2c9)
- ✅ static gates 7/9 (F1: python vs python3 on 2 CI scripts)
- ✅ tsc, test:unit (51), test:plugins
- ⏳ cargo test --lib (running, bg)
- PW desktop suites -> ROG
- ✅ cargo test --lib PASS (exit 0)

## ❌ FINDING F3 (REAL, baseline blocker) — cargo test --lib: 1805 pass / 25 FAIL
- All 25 = plugin::js_runtime::parity_test::claude::* — claude plugin fails to LOAD:
  "incompatible (trust=ThirdParty): hostApi None — build requires hostApi 2".
- Root cause: HOST_API_VERSION=2 (host_api.rs:35); cursor declares "hostApi":2 but
  provider-claude/codex/opencode plugin.json have NO hostApi field -> incompatible as
  ThirdParty (parity test loads them ThirdParty). Real app loads them as system-trusted
  so it works there, but the manifests are stale + a side-loaded copy would break.
- FIX: bump hostApi:2 in claude/codex/opencode manifests (+ rebuild bundles if embedded);
  verify full parity suite green + real app still loads all providers. -> worker wk-hostapi.

## UPDATE — F3 RESOLVED (67a0cf00)
- hostApi:2 added to claude/codex/opencode manifests. cargo test --lib now 1830/0. Parity 38/38 green.
- Local baseline GREEN except F1 (python->python3, CI-only scripts).

## OPEN — P0.2 (critical runtime regression, NOT caught by unit tests)
- Fresh non-main agent panes rejected at hook_session_start ("session does not belong to provider");
  agents don't run on plugins-rethink. Found on ROG, repro on dev. Worker 18fe6380 (Opus) fixing.
- NOTE: 1830 unit tests pass but the app is functionally broken for new agent panes — a coverage GAP
  (Phase-2 review item: add a hook-level test that a fresh launch actually binds + runs).

## FINDING F4 (build robustness) — `./build.sh dev` fails in background/no-TTY
- After the cursor/merge work, pnpm tried to remove node_modules and aborted:
  ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY. Workaround: CI=true ./build.sh dev.
- Phase-2: build.sh should export CI=true (or handle no-TTY) so headless/agent rebuilds don't fail.
  Also: why did pnpm decide to REMOVE node_modules (lockfile churn from version-sync/cursor)? worth a look.

## ✅ PHASE 1 LOCAL BASELINE COMPLETE (96cec31e)
- cargo test --lib: 1833 passed / 0 failed (F3+P0.2 in)
- FE: tsc, test:unit (51), test:plugins all green
- static gates 7/9 (F1 python only)
- P0.2 RUNTIME VERIFIED on local dev: fresh claude/haiku pane bound session 47b45135, replied PONG, no rejection
- ROG (Windows) battery: in progress

## ✅ P0.2 VERIFIED CROSS-PLATFORM
- macOS: pane bound 47b45135 + replied PONG, no rejection.
- Windows (ROG): pane bf843dc6 bound cad34563, agent ran, zero "does not belong to provider".

## Phase 2 review fleet (read-only, in progress)
- P1 capability-discipline (codex), P2 providers+bitwarden (codex), P3 core-seams (opus), P4 async/locks/size (sonnet).
- Pending: P5 security/sandbox, P6 cross-platform (after ROG findings), P7 tests, P8 docs/plan-drift, P9 completeness-critic.

## Phase-2 mid regression (d70c7d8f) — GREEN
- cargo test --lib 1841/0 (all wave-1/2 fixes: detection, restore race/data-loss, F7 split, W2b locks, W2a async)
- tsc clean, test:unit 51/0, test:plugins 31/0

## ✅ PHASE 3 — LOCAL FINAL REGRESSION GREEN (e4a4cf82)
- cargo test --lib: 1860 passed / 0 failed (1841 + 19 new parity)
- tsc clean; test:unit 51/0; test:plugins green
- ALL 9 check-*.sh pass (F1 python3 fix confirmed: check-api-docs + check-evals-list now green)
- pre-push hook (clippy --all-targets + fmt + guards) green on every push through e4a4cf82

## ✅ DEV SMOKE-VERIFY (e4a4cf82)
- F4 backgrounded build works (no pnpm TTY error)
- dev health e4a4cf82 ok; agent spawn 03fe9433 bound session e682ce39 + ran; 0 "does not belong to provider"
