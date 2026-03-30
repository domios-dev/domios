# CodeTerm — Testing Guide

## Three test layers

| Layer | What | Location | Runner | Count |
|-------|------|----------|--------|-------|
| **Unit** | Logic without running app | Rust: `src-tauri/src/`, TS: `src/components/` | `cargo test --lib`, `npx tsx` | 310 |
| **E2E** | API against running instance | `tests/e2e/test_*.py` | `pytest` | 84 |
| **UI** | Real app UI via WebDriver | `tests/webdriver/specs/*.js` | `wdio` | 19 |

## Unit tests

Fast, no running instance needed. Test detection logic, state management, parsers.

```bash
# Rust (210 tests): agent detection, multiplexer state, prompt parsing, config
cargo test --manifest-path src-tauri/Cargo.toml --lib

# TypeScript (100 tests): JSONL parser, terminal output parser, attachment markers
npx tsx src/components/ChatView/parser.test.ts

# Shortcut
./build.sh test   # runs Rust unit tests
```

## E2E tests

Test full stack via REST API against a running dev instance. Create panes, send text, check output, verify prompt lifecycle.

```bash
# Needs running dev instance (./build.sh dev)

# Run all
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/ -v

# Run specific file
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/test_hooks.py -v

# Remote testing (Windows/other machines)
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

Test real app UI via WebDriver against the actual Tauri WebView. Click buttons, verify elements render, test prompt approval flow.

```bash
# Needs dev instance with WebDriver enabled
pnpm tauri dev --config src-tauri/tauri.dev.conf.json -- --features webdriver

# Run tests
cd tests/webdriver && npm test

# Run specific spec
cd tests/webdriver && npx wdio run wdio.conf.js --spec specs/prompt-approval.js
```

### UI test specs

| File | Tests | What |
|------|-------|------|
| `app-loads.js` | 3 | App renders, sidebar, terminal |
| `prompt-approval.js` | 16 | Prompt card, buttons, click Yes/No, two-pane isolation, custom labels |

### UI test setup

Requires `tauri-plugin-webdriver` (embedded WebDriver on port 4445):
- Cargo feature: `--features webdriver` (not included in production)
- Works on all platforms: macOS (WKWebView), Windows (WebView2), Linux (WebKitGTK)

```bash
cd tests/webdriver && npm install  # first time
```

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

<!-- revision: 3 -->
