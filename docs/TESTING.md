# CodeTerm — Testing Guide

## Test-layer taxonomy

This section is the single source of truth for CodeTerm test layers. Other test
docs may describe how to run a suite, but they should link here instead of
redefining the layer model.

For assertion discipline and layer-specific recipes, see `docs/contributing/writing-tests.md`.

| Layer | Lives in | Runner | Real LLM? | Needs dev daemon? | CI | Budget/test |
|---|---|---|---|---|---|---|
| Rust unit | `src-tauri/src/**/tests.rs` | `cargo test --lib` | no | no | yes (D7) | <1s |
| TS unit | `src/**/*.test.ts`, `packages/**` | vitest (`pnpm test:unit`) | no | no | yes (D7) | <1s |
| Plugin | `plugins/*/tests/` | `pnpm test:plugins` | no | no | yes (exists) | <5s |
| API e2e | `tests/e2e/test_*.py` | pytest (`./build.sh e2e`) | no | yes (7686) | spike (D7) | <30s |
| UI smoke | `tests/playwright`, `@smoke` | `pnpm test:pw:smoke` | no | yes | local-only* | <10s |
| UI golden | `tests/playwright`, `@golden` | `pnpm test:pw:golden` | yes | yes | local-only* | <5min |
| Behaviour bench | `evals/<name>/` | `bash run.sh` via `_lib/lib.sh` | yes | yes (dev-locked) | never | unbounded |

\* Playwright stays out of hosted CI by decision because it needs a WebView2
desktop plus host Claude auth. The compensating control is the pre-PR gate.

### Which layer do I write?

```
Is it pure logic (parsing, formatting, state machine)?
├─ yes → unit test next to the code (Rust or TS). STOP — do not write a UI test
│        for something a unit test can prove.
└─ no → Does it require the daemon's HTTP/socket surface?
    ├─ yes, no UI → pytest API e2e
    └─ needs real rendering/interaction →
        ├─ deterministic, no LLM → Playwright @smoke
        ├─ involves a real agent/LLM → Playwright @golden
        └─ measures behaviour quality (prompt change, recall quality,
           routing) rather than pass/fail correctness → evals/ bench,
           NOT a test. Benches compare baselines; tests assert invariants.
```

Rule of thumb: every bug fix gets a test at the lowest layer that can reproduce
it. A cross-pane rendering bug whose root cause is a marker-parsing function gets
a unit test on the parser plus, at most, one golden test for the end-to-end path.

## Runbook

| Layer | Exact command | When required |
|---|---|---|
| Narrow Rust or TS unit | `cargo test <narrow filter>` / `npx tsx <one file>.test.ts` | During work, for the touched area |
| All implemented layers | `./build.sh test:all` | Before claiming done |
| PR gate | `test:all` + `pnpm test:pw:golden` if agent paths touched | Before opening a PR |
| CI gate | Rust unit + TS unit + plugin tests (+ pytest spike) | CI on PR — task D7 |
| Nightly local report | `pnpm test:pw --project=full` + golden | Nightly local cron, report only |

### Unified entrypoint: `./build.sh test:all`

One command runs every non-LLM layer and prints a per-layer summary table. Layers:

1. `rust-unit` — `cargo test --lib` (always runs)
2. `ts-unit` — `pnpm test:unit` → `scripts/ts-unit-tests.sh`, every `src/**` + `packages/**` `*.test.ts` (always runs)
3. `plugin` — `pnpm test:plugins` (always runs)
4. `e2e-pytest` — only if dev (7686) is healthy, else a loud `SKIPPED` line
5. `playwright-smoke` — `pnpm test:pw:smoke`, same dev-up condition

`test:all` exits non-zero **iff an executed layer failed** — a `SKIP` never fails the
run. LLM-judged layers (`evals/`, Playwright `@golden`) are excluded by design; run
them separately. With dev down you still get full unit coverage and a clear record of
what was skipped.

## Unit tests

Fast, no running instance needed. Test detection logic, state management, parsers.

```bash
# Rust (210 tests): agent detection, multiplexer state, prompt parsing, config
cargo test --manifest-path src-tauri/Cargo.toml --lib

# TypeScript: every src/** + packages/** unit test (tsx scripts)
pnpm test:unit
# …or one file while iterating:
npx tsx src/lib/colors.test.ts

# Shortcuts
./build.sh test       # Rust unit tests only
./build.sh test:all   # every non-LLM layer (see Runbook)
```

**Windows: cargo test target isolation**
Running `cargo test` against the app crate while a dev build occupies the same
`target/` directory fails with `Os error 5` — tauri-build cannot overwrite the
in-use `.exe`. Isolate with a separate target directory:

```bash
CARGO_TARGET_DIR=target-test cargo test --manifest-path src-tauri/Cargo.toml --lib
```

## E2E tests

Test full stack via REST API against a running dev instance. Create panes, send text, check output, verify prompt lifecycle.
`./build.sh e2e` skips `llm` tests by default; run `./build.sh e2e:llm` deliberately because that tier starts real model agents and spends provider tokens.

**macOS/Linux:**
```bash
# Needs running dev instance (./build.sh dev)
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/ -v

# Run specific file
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/test_hooks.py -v
```

**Windows (BLACKSTAR):**
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
| `test_panes.py` | 6 | Pane create, split, close, rename |
| `test_pty_io.py` | 13 | PTY read/write, exec, streaming |
| `test_env.py` | 4 | CODETERM_PANE_ID env injection |
| `test_notifications.py` | 3 | Notification create, list |
| `test_cron.py` | 5 | Cron CRUD |
| `test_agents.py` | 5 | Agent hierarchy |
| `test_hooks.py` | 10 | Hook endpoints, prompt parsing accuracy, full approval flow |
| `test_multi_pane.py` | 5 | Multi-pane prompt isolation |
| `test_attachments.py` | 3 | File attachment via API |
| `test_logging.py` | 4 | Log file, levels |
| `test_claude_spawn.py` | 8 | Real Claude spawn (**slow**, 30-90s each) |
| `test_hierarchy_scenarios.py` | 15 | Multi-level hierarchies (**slow**) |

### E2E dependencies
```bash
pip install -r tests/e2e/requirements.txt  # requests, pytest
```

## UI tests

Test real app UI with Playwright against the actual Tauri dev console. On
Windows, CodeTerm's WebView2 window exposes Chrome DevTools Protocol on port
9223; Playwright attaches with `chromium.connectOverCDP`, so tests drive the
real Tauri WebView, real IPC, and real daemon state.

```bash
./build.sh dev --wait
pnpm test:pw:lint
pnpm test:pw:smoke
pnpm test:pw:golden
pnpm test:pw:full
```

Operational details live in `tests/playwright/README.md`: CDP setup,
projects/tags, golden-test policy, web-mirror mode, and prod guards.

The legacy Tauri WebDriver/WDIO harness was removed; do not add new WebDriver
tests. For browser-mode `/web` coverage, use the `tests/playwright/suites/12-web`
suite, which launches a normal browser against the daemon's web mirror.

## Cross-platform test matrix

One command runs the unit layers (rust-unit, cli, ts-unit, plugin) for all three
OS targets in parallel from a macOS host:

```bash
./scripts/test-matrix.sh                 # all lanes
./scripts/test-matrix.sh linux           # single lane
```

| Lane | Where | What runs | Coverage |
|------|-------|-----------|----------|
| `macos` | native host | `cargo test --lib` (src-tauri) + cli tests + `pnpm test:unit` + `pnpm test:plugins` | full execution |
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
# Windows (BLACKSTAR)
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

1. **PTY output arrives** -- `OutputBuffer` appends raw bytes and feeds them into a per-pane VT100 parser (`vt100` crate).
2. **`process_pty_output()`** (`src-tauri/src/service/output.rs`) calls `has_event_markers()` on the cleaned chunk text. This is a cheap string check that gates the expensive path.
3. **If markers found** -- `get_screen_text()` on `OutputBuffer` returns the actual terminal screen content (what the user sees), correctly handling cursor positioning, alternate screen, and TUI overlays.
4. **`PromptDetector.evaluate_pty_output()`** (`src-tauri/src/agent/prompt.rs`) runs the state machine. It delegates to `parse_permission_prompt()` for structural fingerprint detection.
5. **If prompt found** -- returns a `PromptDecision::NewPrompt` or `PromptDecision::PromptUpdated`. `apply_prompt_decision()` sets `pending_prompt` on the pane and emits `mux-state-changed`.
6. **Frontend** (`ChatView.tsx`) polls mux state, sees the pending prompt, and renders Yes/No buttons. User clicks a button, which calls `mux_respond_prompt()` and sends the chosen keystroke to the PTY.

### Key source files

| File | Role |
|------|------|
| `src-tauri/src/agent/prompt.rs` | `PromptDetector` state machine (Idle -> Active -> Cooldown -> Idle) |
| `src-tauri/src/agent/detector.rs` | Structural fingerprint detection (`detect_interactive_prompt`, `parse_permission_prompt`) + event detection (`detect_claude_events`) |
| `src-tauri/src/pty/manager.rs` | `OutputBuffer` with per-pane VT100 parser, `get_screen_text()` |
| `src-tauri/src/service/output.rs` | `process_pty_output()` orchestration + `apply_prompt_decision()` |
| `src/components/ChatView/ChatView.tsx` | Frontend polling + button rendering |

### State machine: PromptDetector

Each pane has independent state:

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
| **PTY auto-detect** | `process_pty_output()` -> `PromptDetector.evaluate_pty_output()` | Always runs for agent panes; reliable fallback |
| **Hook (API)** | `POST /api/panes/{id}/hook/permission` -> `PromptDetector.hook_detected_explicit()` | Claude Code hooks call this; faster, bypasses cooldown |

Both paths converge on the same `PromptDetector` state machine and `apply_prompt_decision()`.

### Testing prompt detection

Unit tests cover the full matrix:

```bash
# PromptDetector state machine (14 tests)
cargo test --manifest-path src-tauri/Cargo.toml --lib prompt::tests

# Structural fingerprint detection (30+ tests)
cargo test --manifest-path src-tauri/Cargo.toml --lib detector::tests
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

<!-- revision: 11 -->
