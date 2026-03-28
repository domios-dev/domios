# CodeTerm — Testing Guide

## Test Layers

| Layer | Location | Runner | Count | What it tests |
|-------|----------|--------|-------|---------------|
| **Rust unit** | `src-tauri/src/**/mod.rs` | `cargo test --lib` | 205 | Agent detection, multiplexer state, service logic, config parsing |
| **Parser unit** | `src/components/ChatView/parser.test.ts` | `npx tsx ...test.ts` | 100 | JSONL + terminal output parsing, attachment markers |
| **E2E API** | `tests/e2e/test_*.py` | `pytest` | 81 | Full stack via REST API against running instance |

## Running Tests

### Rust unit tests (fast, no instance needed)
```bash
./build.sh test
# or
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

### TypeScript parser tests (fast, no instance needed)
```bash
npx tsx src/components/ChatView/parser.test.ts
```

### E2E tests (needs running dev instance)
```bash
# Option 1: Auto-start instance (slow first run — compiles)
cd tests/e2e && python3 -m pytest -v

# Option 2: Use already running instance (faster)
CODETERM_API_URL=http://127.0.0.1:7686 python3 -m pytest tests/e2e/ -v

# Run specific test file
CODETERM_API_URL=http://127.0.0.1:7686 python3 -m pytest tests/e2e/test_prompt_detection.py -v

# Run specific test
CODETERM_API_URL=http://127.0.0.1:7686 python3 -m pytest tests/e2e/test_claude_spawn.py::test_spawn_claude_basic -v
```

### E2E dependencies
```bash
pip install -r tests/e2e/requirements.txt  # requests, pytest
```

## E2E Test Files

| File | Tests | What it covers |
|------|-------|----------------|
| `test_health.py` | 2 | API health, identify endpoint |
| `test_workspaces.py` | 4 | Workspace CRUD |
| `test_panes.py` | 6 | Pane create, split, close, rename, focus |
| `test_pty_io.py` | 13 | PTY read/write, exec, output streaming |
| `test_env.py` | 4 | CODETERM_PANE_ID, CODETERM_SOCKET_PATH env injection |
| `test_notifications.py` | 3 | Notification create, list, read |
| `test_cron.py` | 5 | Cron job CRUD |
| `test_agents.py` | 5 | Agent hierarchy: spawn, roles, reports |
| `test_claude_spawn.py` | 8 | **Slow** — real Claude spawn, task delivery, hierarchy cascade |
| `test_hierarchy_scenarios.py` | 15 | Multi-level agent hierarchies, real tasks |
| `test_prompt_lifecycle.py` | 16 | Hook endpoints, multi-pane prompts, file attachments, prompt lifecycle, logging |

## E2E Architecture

### How it works
1. Tests connect to a running CodeTerm instance via REST API
2. `conftest.py` handles instance lifecycle (auto-start if needed)
3. `helpers.py` provides `CodeTermAPI` client with convenience methods
4. `fresh_pane` fixture creates a disposable workspace + pane per test
5. Tests use API to create panes, send commands, check output/screen/state

### API Token
- Dev config: `~/.codeterm-dev/config.toml` → `[api]` section → `token`
- Loaded automatically by `conftest.py`

### Key helpers
```python
api.exec_cmd(pane_id, "echo hello")      # Execute command, wait for output
api.send_text(pane_id, text="hello")     # Send raw text to PTY
api.get_output(pane_id)                   # Get raw/stripped output buffer
api.get_screen(pane_id)                   # Get VT100-rendered visible screen
api.wait_for_output(pane_id, "pattern")  # Poll until pattern appears
api.wait_for_screen(pane_id, "pattern")  # Poll screen until pattern
```

### Slow tests (Claude spawn)
Tests in `test_claude_spawn.py` and `test_hierarchy_scenarios.py` spawn real Claude Code instances. They take 30-90 seconds each. Run separately:
```bash
CODETERM_API_URL=http://127.0.0.1:7686 python3 -m pytest tests/e2e/test_claude_spawn.py -v -x
```

## Logging

- **Dev logs**: `~/.codeterm-dev/logs/codeterm.log`
- **Prod logs**: `~/.codeterm/logs/codeterm.log`
- **Levels**: DEBUG (dev), INFO (prod)
- **Rotation**: 5MB max, keeps 3 rotated files
- **View live**: `tail -f ~/.codeterm-dev/logs/codeterm.log`
- **Search errors**: `grep ERROR ~/.codeterm-dev/logs/codeterm.log`

## What's NOT tested yet

- **UI rendering** — chat view, message bubbles, prompt buttons (needs Playwright/Tauri test driver)
- **Image paste** — clipboard interaction can't be tested via API
- **Cross-platform** — tests only run on macOS currently
- **Multiple agent providers** — only Claude tested, Codex/Aider providers not implemented yet
- **Session restore with active Claude** — needs restart during test

## Adding new tests

1. API-level tests → add to appropriate `test_*.py` or create new file
2. Rust unit tests → add `#[test]` in the relevant module's `mod tests`
3. Parser tests → add to `parser.test.ts`
4. Always run `./build.sh test` + e2e before release

<!-- revision: 1 -->
