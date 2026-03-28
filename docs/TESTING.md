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
CODETERM_API_URL=http://100.84.159.37:7685 CODETERM_API_TOKEN=TOKEN pytest tests/e2e/ -v
```

Known platform differences:
- Shell: PowerShell on Windows (`$env:VAR`), bash/zsh on macOS/Linux (`$VAR`)
- Paths: `C:\Users\` vs `/Users/`
- Some PTY I/O tests are bash-specific

## What's NOT tested

- Clipboard/image paste (needs OS-level interaction)
- Cross-platform PTY differences (PowerShell vs bash)
- Session restore with active Claude across restart
- Real Claude permission prompt → chat UI → approve → Claude continues (needs real Claude running)

<!-- revision: 2 -->
