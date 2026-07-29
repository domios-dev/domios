# CodeTerm — Testing Guide

## Test-layer taxonomy

This section is the single source of truth for CodeTerm test layers. Other test
docs may describe how to run a suite, but they should link here instead of
redefining the layer model.

For assertion discipline and layer-specific recipes, see `docs/contributing/writing-tests.md`.

| Layer | Lives in | Runner | Real LLM? | Needs dev daemon? | CI | Budget/test |
|---|---|---|---|---|---|---|
| Rust unit | `src-tauri/src/**/tests.rs` | `scripts/test-capped.sh … --lib` | no | no | yes (D7) | <1s |
| TS unit | `src/**/*.test.ts`, `packages/**` | tsx scripts (`npm run test:unit`) | no | no | yes (D7) | <1s |
| Plugin | `plugins/*/tests/` | `pnpm test:plugins` | no | no | yes (exists) | <5s |
| API e2e | `tests/e2e/test_*.py` | pytest (`./build.sh e2e`) | no | yes (7686) | spike (D7) | <30s |
| Behaviour bench | `evals/<name>/` | `bash run.sh` via `_lib/lib.sh` | yes | yes (dev-locked) | never | unbounded |

Playwright is suspended and is not part of the test plan, agent workflow, local
aggregate, QA registry, or CI. Existing specs remain archived for a future
explicit run requested by Andrey in the current conversation; do not run or
modify them during normal development.

### Which layer do I write?

```
Is it pure logic (parsing, formatting, state machine)?
├─ yes → unit test next to the code (Rust or TS). STOP — do not write a UI test
│        for something a unit test can prove.
└─ no → Does it require the daemon's HTTP/socket surface?
    ├─ yes, no UI → pytest API e2e
    └─ needs real rendering/interaction → focused unit/API coverage plus manual
        dev inspection; Playwright remains suspended. Behaviour-quality changes
        use an eval bench, not a pass/fail UI test.
```

Rule of thumb: every bug fix gets a test at the lowest layer that can reproduce
it. A cross-tab rendering bug whose root cause is a marker-parsing function gets
a unit test on the parser plus, at most, one golden test for the end-to-end path.

## Runbook

| Layer | Exact command | When required |
|---|---|---|
| Narrow Rust or TS unit | `scripts/test-capped.sh … <filter>` / `npx tsx <one file>.test.ts` | During work, for the touched area |
| All implemented layers | `./build.sh test:all` | Before claiming done |
| PR gate | `./build.sh test:all` | Before opening a PR |
| CI gate | Rust unit + TS unit + plugin tests (+ pytest spike) | CI on PR — task D7 |

### Unified entrypoint: `./build.sh test:all`

One command runs every non-LLM layer and prints a per-layer summary table. Layers:

1. `rust-unit` — capped `cargo test --lib` through `scripts/test-capped.sh` (always runs)
2. `ts-unit` — `npm run test:unit` → `scripts/ts-unit-tests.sh`, every `src/**` + `packages/**` `*.test.ts` (always runs)
3. `plugin` — `pnpm test:plugins` (always runs)
4. `e2e-pytest` — only if dev (7686) is healthy, else a loud `SKIPPED` line

`test:all` exits non-zero **iff an executed layer failed** — a `SKIP` never fails the
run. Behaviour benches and the suspended Playwright suite are excluded. With dev
down you still get full unit coverage and a clear record of
what was skipped.

The canonical dev monitor owns the machine build gateway while port 7686 is
running. Stop dev with `./build.sh kill` before `test:all`; otherwise the gate
queues behind dev and cannot start. If its caller exits while queued, cancel the
orphan with `./build.sh gateway-cancel <job-id>` before retrying.

## Unit tests

Fast, no running instance needed. Test detection logic, state management, parsers.

### Launch-contract harness

The launch-contract harness is a focused Cargo test set. It does not require a
dev daemon or an app bundle; run each oracle under the capped runner:

```bash
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::role_identity::tests::compose_orchestrator_golden -- -- --exact
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::role_identity::tests::compose_planner_golden -- -- --exact
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::spawn_params::tests::worker_launch_has_no_manager_bootstrap -- -- --exact
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml -p codeterm-plugin --lib js_runtime::parity_test::launch_adapter_matrix_uses_each_installed_descriptor -- -- --exact
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::launch_delivery::tests::delivery_prefers_system_prompt_when_declared -- -- --exact
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::launch_delivery::tests::system_prompt_channel_consumes_launch_content_without_a_fallback_turn -- -- --exact
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::launch_delivery::tests::external_provider_defers_identical_content_to_confirmed_fallback -- -- --exact
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib submission_matrix::claude -- -- --nocapture
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib submission_matrix::codex -- -- --nocapture
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib submission_matrix::opencode -- -- --nocapture
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib submission_matrix::cursor -- -- --nocapture
```

The matrix reads each provider's `readinessStrategy`, `verifySubmit`, and
`hasHooks` capabilities from `plugins/provider-*/plugin.json`. Claude, Codex,
and OpenCode run all sixteen dimensions. Cursor runs readiness plus byte-exact
ASCII, Unicode, and 5000-character multiline ingestion with zero submit writes;
every submit-dependent dimension prints a quota-constraint skip reason.

The frontend key and delivery-ledger oracles are focused and bounded:

```bash
npm run test:unit -- ChatComposer.submission-keys
npm run test:unit -- chatSendStatus
npm run test:unit -- TabSessionChat.delivery
```

The provider-row harness verifies the real `codeterm` CLI before running those
focused oracles. Its help and dry-run surface are:

```bash
bash scripts/submission-matrix.sh --help
bash scripts/submission-matrix.sh --provider claude --dry-run
```

```bash
# Rust: agent detection, multiplexer state, prompt parsing, config
scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib

# TypeScript: every src/** + packages/** unit test (direct Node scheduler, 90s outer cap)
bash -lc 'timeout --signal=TERM --kill-after=5s 90s npm run test:unit'
# …or one file through the same bounded scheduler:
npm run test:unit -- colors.test

# Shortcuts
./build.sh test       # Rust unit tests only
./build.sh test:all   # every non-LLM layer (see Runbook)
```

### Chat attach/hydration lifecycle

Use the deterministic hook oracle for Terminal → Chat activation, tab-owned
authority, bounded hydration, resync, and subscription cleanup:

```bash
npx tsx src/components/Terminal/useChatSession.attachHydration.test.tsx
npx tsx src/components/Terminal/TabContent.chatContinuity.test.tsx
npx tsx src/components/Terminal/ChatSetupState.test.tsx
```

The harness starts hidden with null Tab authority, activates into
`awaiting_session`, and proves a global/foreign candidate cannot create Chat
authority. It then commits the Tab's own session id and binding nonce while
hidden, reactivates, and requires the pre-existing transcript exactly once with
one subscription. Its resync replaces the subscription without duplicating
rows; wrong-tab, wrong-session, wrong-nonce, cancelled-generation, and
prior-lifecycle inputs remain rejected. The setup-state oracle proves a
hydrated current-authority transcript suppresses stale `shell_ready` and earlier
launch overlays. The `TabContent` oracle refreshes the mux with a new Tab
snapshot carrying the same provider, model, session id, and binding nonce, then
drives Terminal → Chat and requires one pre-existing row and one subscription
without losing the mode switch or identity labels.

### Capped Rust test preflight

`scripts/test-capped.sh` is the required lane-local Rust-test wrapper. In a
fresh worktree, it creates the ignored, zero-byte `bundled-embed` sidecar only
when that sidecar is absent, then runs the requested test under its normal hard
process-group cap. The wrapper logs that one test-only repair once; a later run
and a real canonical sidecar are no-ops. The bundle script rejects empty
sidecars, so this preflight cannot produce a release artifact.

Verify the wrapper itself with the focused shell test:

```bash
bash scripts/test-capped.test.sh
```

**Windows: cargo test target isolation**
Running the capped Rust test runner against the app crate while a dev build occupies the same
`target/` directory fails with `Os error 5` — tauri-build cannot overwrite the
in-use `.exe`. The required wrapper detects a healthy dev instance and
automatically routes focused tests to the worktree-local
`target-check` cache and enables machine-level `sccache` before Cargo starts:

```bash
TEST_CAP_SECS=90 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib <filter>
```

### REST generation liveness

Run the listener-exit and worker-starvation fixtures separately:

```bash
bash scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib startup::rest_api::tests::unexpected_exit_autonomously_replaces_generation_after_health_proof -- --exact
bash scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib startup::rest_api::tests::diagnostics_distinguish_listener_exit_from_worker_stall -- --exact
bash scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib startup::rest_api::tests::repeated_same_generation_outages_emit_exactly_one_resync_each -- --exact
```

The first fixture sends no manual recovery command and observes lifecycle
events through the replacement's real `/api/health` response. The second uses
explicit barriers to hold all four REST workers while leaving the listener
bound, asserts an unanswered health request within a bounded failure deadline,
and verifies that its diagnostic differs from listener exit without publishing
a false healthy state. This worker gate is synthetic diagnostic coverage, not
the incident cause. The third fixture proves two stalls in one generation each
claim one distinct recovery resync and duplicate observation claims none.

Run the real-load dev oracle separately:

```bash
bash evals/api-liveness-mesh-load/run.sh
```

It is locked to dev 7686 through `evals/_lib/lib.sh` and `dc`, has a 15-second
failure deadline, and issues no readiness poll or delay. One barrier releases
four sparse 476 MiB Claude transcript reads, health/tab observations, signed
report/heartbeat-shaped relays, a duplicate relay, and a structured-error
relay. It requires health/tab within one second, relay/send acknowledgement
within two seconds, one durable delivery per id, and the original structured
delivery error. The real load owner is the session parser; the bounded 4 MiB
tail and blocking-worker route keep it off the four REST runtime workers.
Neither the fixtures nor the oracle address production port 7685.

## E2E tests

Test full stack via REST API against a running dev instance. Create tabs, send text, check output, verify prompt lifecycle.
`./build.sh e2e` skips `llm` tests by default; run `./build.sh e2e:llm` deliberately because that tier starts real model agents and spends provider tokens.

**macOS/Linux:**
```bash
# Needs running dev instance (./build.sh dev)
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/ -v

# Run specific file
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/test_hooks.py -v
```

**Windows:**
```bash
./build.sh dev --wait       # starts dev on 7686 and leaves production on 7685 alone
bash dev.local.sh dev       # local fallback: starts dev and waits for API on 7686
bash dev.local.sh test      # runs pytest against dev instance
bash dev.local.sh all       # dev + wait + test
```

**Remote testing:**
```bash
CODETERM_API_URL=http://HOST:PORT CODETERM_API_TOKEN=TOKEN pytest tests/e2e/ -v
```

### E2E test files

| File | Tests | What |
|------|-------|------|
| `test_health.py` | 2 | API health, identify |
| `test_workspaces.py` | 4 | Workspace CRUD |
| `test_tabs.py` | 6 | Tab create, split, close, rename |
| `test_pty_io.py` | 13 | PTY read/write, exec, streaming |
| `test_env.py` | 4 | CODETERM_TAB_ID env injection |
| `test_notifications.py` | 3 | Notification create, list |
| `test_cron.py` | 5 | Cron CRUD |
| `test_agents.py` | 5 | Agent hierarchy |
| `test_hooks.py` | 10 | Hook endpoints, prompt parsing accuracy, full approval flow |
| `test_multi_tab.py` | 5 | Multi-tab prompt isolation |
| `test_attachments.py` | 3 | File attachment via API |
| `test_logging.py` | 4 | Log file, levels |
| `test_claude_spawn.py` | 8 | Real Claude spawn (**slow**, 30-90s each) |
| `test_hierarchy_scenarios.py` | 15 | Multi-level hierarchies (**slow**) |

### E2E dependencies
```bash
pip install -r tests/e2e/requirements.txt  # requests, pytest
```

## UI tests

Automated UI tests are currently suspended. Do not invoke or extend the archived
Playwright suite. Cover UI logic at the TypeScript unit boundary, cover daemon
behavior through Rust or API tests, preserve real accessibility semantics, and
perform focused manual inspection in dev when rendering is the only useful oracle.

## Cross-platform test matrix

One command runs the unit layers (rust-unit, cli, ts-unit, plugin) for all three
OS targets in parallel from a macOS host:

```bash
./scripts/test-matrix.sh                 # all lanes
./scripts/test-matrix.sh linux           # single lane
```

| Lane | Where | What runs | Coverage |
|------|-------|-----------|----------|
| `macos` | native host | capped `cargo test --lib` (src-tauri) + cli tests + `pnpm test:unit` + `pnpm test:plugins` | full execution |
| `linux` | Docker (ubuntu:24.04, aarch64) | same suites | full execution, isolated |
| `windows-cross` | Docker + cargo-xwin | `cargo check --target x86_64-pc-windows-msvc --lib --tests` (src-tauri + cli) | compile-level only |

Windows containers cannot run on a macOS Docker host, so full Windows test
**execution** is delegated to a real Windows machine on the CodeTerm mesh
(cross-compiled binaries built locally via `cargo xwin test --no-run`, test job
dispatched to the peer) — or to remote E2E (`CODETERM_API_URL=... pytest`). The
local `windows-cross` lane still catches `#[cfg(windows)]` compile breakage in
minutes without leaving the machine.

All lanes run cargo with `--locked`: lanes share one checkout via bind-mount, and
a lane rewriting `Cargo.lock` while another reads it produces torn reads through
VirtioFS. Keep the lockfile committed up-to-date or the lanes fail loudly.

Logs land in `$TMPDIR/codeterm-test-matrix/<lane>.log`. Containers shadow
`src-tauri/target`, `cli/target`, and `node_modules` with named Docker volumes
(`codeterm-linux-*`, `codeterm-xwin-*`) — the host checkout is never dirtied,
and warm reruns are incremental. To reclaim disk:
`docker volume rm $(docker volume ls -q | grep '^codeterm-')`.

### Cross-platform CI troubleshooting

- A Tauri test harness that must populate a production `AppHandle` uses
  `tauri::Builder::default()`. `mock_builder()` changes the runtime type. A
  libtest-thread harness uses `any_thread()` on Windows; macOS requires the
  process main thread and headless Linux cannot initialize GTK, so the real
  AppHandle oracle runs on Windows while pure lifecycle tests cover every OS.
- Real-socket tests that reserve an ephemeral address and then rebind it must
  serialize that reservation inside the test module. Parallel bind-drop-bind
  fixtures can claim each other's released ports, especially on Windows.
- Run `scripts/clippy-gate.sh` before pushing. Toolchain lint changes are release
  failures even when focused tests and `cargo check` pass.

## Logging

| | Path |
|---|---|
| Dev | `~/.codeterm-dev/logs/codeterm.log` |
| Prod | `~/.codeterm/logs/codeterm.log` |

- Levels: DEBUG (dev), INFO (prod)
- Rotation: 5MB max, 3 files kept
- Live view: `tail -f ~/.codeterm-dev/logs/codeterm.log`

## Cross-platform testing

Run E2E tests against remote machines:
```bash
# Windows
CODETERM_API_URL=http://<REMOTE_IP>:<PORT> CODETERM_API_TOKEN=<TOKEN> pytest tests/e2e/ -v
```

Known platform differences:
- Shell: PowerShell on Windows (`$env:VAR`), bash/zsh on macOS/Linux (`$VAR`)
- Paths: `C:\Users\` vs `/Users/`
- Some PTY I/O tests are bash-specific

## Manual paste smoke matrix

Paste paths must be re-checked manually on each platform where Playwright
cannot exercise the OS clipboard path directly. All paths route through
`src/lib/clipboardPaste.ts` — file/image classification is shared,
so a regression in one path usually means a regression in all.

| Input | macOS (Cmd) | Windows (Ctrl) | Linux (Ctrl) |
|-------|-------------|----------------|--------------|
| Plain text via Ctrl+V / Cmd+V | echoed (bracketed) | echoed (bracketed) | echoed (bracketed) |
| Long text (>1MB) via Ctrl+V | no UI freeze, no truncation | same | same |
| Ctrl+Shift+V | same as Ctrl+V | same as Ctrl+V | same as Ctrl+V |
| Right-click → Paste | same as Ctrl+V | same as Ctrl+V | same as Ctrl+V |
| Drag plain text from another app | bracketed text in PTY | same | same |
| Drag a single file from Finder/Explorer | quoted absolute path | quoted absolute path | quoted absolute path |
| Drag multiple files | space-separated quoted paths | same | same |
| Drag a folder | quoted folder path | same | same |
| Drag multiple folders | space-separated quoted paths | same | same |
| PNG image on clipboard via Ctrl+V | temp-file path (`.png`) | same | same |

Behaviour rules locked in code:
- Plain text is always wrapped in `\x1b[200~...\x1b[201~` (bracketed paste)
  via `sendPlainTextToPty` — the same envelope on every path so TUIs
  collapse it to a single `[Pasted text #N]` placeholder.
- File / folder paths are joined with single space and quoted by
  `quotePathForPaste` (single quotes by default, double quotes if the
  path itself contains a single quote, with `"\$\``" escaped).
- Image bytes are persisted via the `save_clipboard_image` Tauri
  command, then the saved path is pasted.
- A non-empty native clipboard file list wins over any plain text on
  the clipboard — paste behaves "as if you dragged the files".

## Prompt Detection Architecture

The permission prompt detection pipeline detects when AI agents show interactive prompts (e.g., "Do you want to proceed? 1. Yes 2. No") and renders clickable approval buttons in the chat UI.

### Pipeline

1. **PTY output arrives** -- `OutputBuffer` appends raw bytes and feeds them into a per-tab VT100 parser (`vt100` crate).
2. **`process_pty_output()`** (`src-tauri/src/service/output.rs`) calls `has_event_markers()` on the cleaned chunk text. This is a cheap string check that gates the expensive path.
3. **If markers found** -- `get_screen_text()` on `OutputBuffer` returns the actual terminal screen content (what the user sees), correctly handling cursor positioning, alternate screen, and TUI overlays.
4. **`PromptDetector.evaluate_pty_output()`** (`src-tauri/src/agent/prompt.rs`) runs the state machine. It delegates to `parse_permission_prompt()` for structural fingerprint detection.
5. **If prompt found** -- returns a `PromptDecision::NewPrompt` or `PromptDecision::PromptUpdated`. `apply_prompt_decision()` sets `pending_prompt` on the Tab and emits `mux-state-changed`.
6. **Frontend** (`TabSessionChat.tsx`) reads mux state, sees the pending prompt, and renders response controls. User responds, which calls `mux_respond_prompt()` and sends the chosen keystroke to the PTY.

### Key source files

| File | Role |
|------|------|
| `src-tauri/src/agent/prompt.rs` | `PromptDetector` state machine (Idle -> Active -> Cooldown -> Idle) |
| `src-tauri/src/agent/detector.rs` | Structural fingerprint detection (`detect_interactive_prompt`, `parse_permission_prompt`) + event detection (`detect_claude_events`) |
| `src-tauri/src/pty/manager.rs` | `OutputBuffer` with per-tab VT100 parser, `get_screen_text()` |
| `src-tauri/src/service/output.rs` | `process_pty_output()` orchestration + `apply_prompt_decision()` |
| `src/components/Terminal/TabSessionChat.tsx` | Frontend prompt state + response controls |

### State machine: PromptDetector

Each tab has independent state:

```
Idle ──[prompt detected]──> Active ──[user responds]──> Cooldown ──[500ms]──> Idle
                               │                                                │
                               ├──[same options]──> (no-op)                     │
                               └──[different options]──> PromptUpdated          │
                                                                                │
Cooldown ──[hook arrives]──> Active  (hooks bypass cooldown)                    │
```

- **Idle**: No active prompt. Output is parsed for prompts.
- **Active**: Prompt is shown to the user. Same-options output is suppressed. Changed options trigger `PromptUpdated`.
- **Cooldown**: User responded (500ms). PTY auto-detection is suppressed to avoid re-detecting the same prompt before the agent clears it. Hooks (API path) bypass cooldown since they are authoritative.

### Structural fingerprint detection (false positive prevention)

Detection uses structural patterns, never hardcoded question text. This makes it agent-agnostic and resilient to wording changes.

**Requirements for a positive match:**
- 2+ numbered options in `N. label` format (e.g., `1. Yes`, `2. No`)
- PLUS at least one of:
  - A `>` or `❯` selector prefix on one of the options
  - A keyboard hint footer containing `Esc`/`Enter` with `cancel`/`amend`/`confirm`
- Options and footer/selector must be within **8 lines** proximity (prevents matching a numbered list in the answer that sits far above the actual prompt)
- Only the last **20 lines** of screen content are scanned (prompt is always at the bottom)

**What does NOT match:**
- Plain numbered lists (no selector or footer)
- Source code containing prompt-like strings (wrong structure)
- Numbered log timestamps
- Markdown numbered lists in agent answers

### Idle detection and status tracking

The status tracker (`agent_tracker`) determines whether an agent is Working, Waiting, or Idle:

- Uses a **screen content hash** (SHA-256 of `get_screen_text()`) instead of raw PTY bytes
- Cursor blinks and TUI redraws (ANSI noise) do not change the screen content hash, so they do not reset the idle timer
- Default idle timeout: **5 seconds** of unchanged screen content

### Two detection paths

| Path | Entry point | When used |
|------|-------------|-----------|
| **PTY auto-detect** | `process_pty_output()` -> `PromptDetector.evaluate_pty_output()` | Always runs for agent Tabs; reliable fallback |
| **Hook (API)** | `POST /api/tabs/{id}/hook/permission` -> `PromptDetector.hook_detected_explicit()` | Claude Code hooks call this; faster, bypasses cooldown |

Both paths converge on the same `PromptDetector` state machine and `apply_prompt_decision()`.

### Testing prompt detection

Unit tests cover the full matrix:

```bash
# PromptDetector state machine (14 tests)
scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib prompt::tests

# Structural fingerprint detection (30+ tests)
scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib detector::tests
```

Key test categories in `detector.rs`:
- **True positives**: real Claude prompts (bash, file write, read), generic TUI menus
- **False positives**: numbered answers, bug analyses, step instructions, code reviews, phased plans, comparison lists, security audits
- **Edge cases**: prompt buried after long output, old prompt scrolled off, question mark before plain list, `Esc` text far from list

E2E tests (`test_hooks.py`) verify the full lifecycle: hook -> pending_prompt set -> respond -> prompt cleared.

UI tests (`prompt-approval.js`) verify button rendering and click behavior.

## What's NOT tested

- Clipboard/image paste (needs OS-level interaction)
- Cross-platform PTY differences (PowerShell vs bash)
- Session restore with active Claude across restart
- Real Claude permission prompt → chat UI → approve → Claude continues (needs real Claude running)

## Reviewer's checklist

Before approving a PR, use **`docs/contributing/review-checklist.md`** to verify evidence discipline, test methodology, documentation updates, and code quality.

<!-- revision: 30 -->
