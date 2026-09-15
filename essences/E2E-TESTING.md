# End-to-end testing

E2E, UI, cross-platform, logging, and manual smoke workflows.

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

<!-- revision: 2 -->
