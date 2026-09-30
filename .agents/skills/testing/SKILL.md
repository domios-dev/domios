---
name: testing
description: "Choose, author, and run CodeTerm unit, integration, REST, and cross-platform tests."
---

## CodeTerm — Testing Guide

## Test-layer taxonomy

This section is the single source of truth for CodeTerm test layers. Other test
docs may describe how to run a suite, but they should link here instead of
redefining the layer model.

For assertion discipline and layer-specific recipes, see `the Writing Tests section of this skill`.

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

1. `rust-unit` — `cargo test --lib` through `scripts/test-capped.sh` with its 600-second default cap (always runs)
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
## Rust: agent detection, multiplexer state, prompt parsing, config
scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib

## TypeScript: every src/** + packages/** unit test (direct Node scheduler)
npm run test:unit
## …or one file through the same scheduler:
npm run test:unit -- colors.test

## Shortcuts
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

### Rust test wrapper preflight

`scripts/test-capped.sh` is the required Rust-test wrapper. Focused invocations
default to 600 seconds; `build.sh test` and `verify` set 3600 seconds for each
Cargo test invocation. Cargo first compiles the same targets with `--no-run`,
outside the execution deadline. `TEST_CAP_SECS=<positive-seconds>` overrides
either deadline; `TEST_CAP_SECS=0` disables it. On expiry the wrapper kills the
execution process group, exits 124, and repeats libtest's active case names from
its captured output. Stable Rust cannot use `--ensure-time`: it is nightly-only
and reports a long test only after it finishes, so the process deadline owns hang
recovery. In a fresh worktree, the wrapper also creates the ignored, zero-byte
`bundled-embed` sidecar only when absent. The bundle script rejects empty
sidecars.

The TypeScript scheduler still waits for every file and the full suite by
default. Set `TS_UNIT_TIMEOUT_SECONDS=<positive-seconds>` or
`TS_UNIT_SUITE_TIMEOUT_SECONDS=<positive-seconds>` for an explicit diagnostic
deadline; ordinary `npm run test:unit` has neither implicit limit.

On Linux, a repository-watcher silence test that reports existing `refs/`
directories is an inotify access event from recursive watch registration, not
repository churn. Keep access filtering upstream of the debouncer, then rerun
`repository_watcher_stays_silent_on_irrelevant_churn`; never add a settle wait.

**Filtered runs fail closed.** A filtered run that executed zero tests exits `97`
naming the filter and the crate: a filter that ran nothing verified nothing.
Unfiltered runs are never checked, so an empty target still passes. Verify the
wrapper whole, or one named case from `scripts/test-capped.test.sh`:

```bash
bash scripts/test-capped.test.sh [case]
```

**Windows: ConPTY test isolation**
A test process launched from a CodeTerm tab inherits `CODETERM_CONPTY_DIR` and would sideload the installed host into the user's install. `scripts/test-capped.sh` unsets it and exports `CODETERM_CONPTY_TEST_ISOLATION=1`; `pty-fork/src/win/psuedocon.rs` refuses the variable while that marker is set, and production never sets it. Verify: `./build.sh test-one pty-fork test_isolation_marker_refuses_ambient_production_dir`.

**Windows: cargo test target isolation**
Running the Rust test wrapper against the app crate while a dev build occupies the same
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

## Plugin test launcher troubleshooting

`npm run test:plugins` uses the repository launcher, which selects Git Bash on
Windows and `bash` elsewhere. If it reports that Git Bash is missing, install
Git for Windows at its standard location and rerun the same command.

## Related documents

End-to-end and cross-platform verification: `.agents/skills/testing/SKILL.md`. Prompt detection: `codeterm docs PROMPT-DETECTION`. Test design rules: `codeterm docs TEST-AUTHORING`.

---

## End-to-end testing

E2E, UI, cross-platform, logging, and manual smoke workflows.

## E2E tests

Test full stack via REST API against a running dev instance. Create tabs, send text, check output, verify prompt lifecycle.
`./build.sh e2e` skips `llm` tests by default; run `./build.sh e2e:llm` deliberately because that tier starts real model agents and spends provider tokens.

**macOS/Linux:**
```bash
## Needs running dev instance (./build.sh dev)
CODETERM_API_URL=http://127.0.0.1:7686 pytest tests/e2e/ -v

## Run specific file
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
## Windows
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

---

## Writing Tests

This section covers test shape, assertion discipline, and review expectations
after selecting a layer in the taxonomy above. Plugin parser changes need plugin
tests and native parity fixtures where available. API end-to-end tests default
to non-LLM; provider-token cases are marked and run deliberately.
## Honesty rules

Rule: prove command output with split markers. WHY: a terminal echo of the typed
line can satisfy a naive substring assert. Example:
`tests/e2e/test_pty_io.py` builds `echo 'LEFT''RIGHT'`; on Windows use
`Write-Output ('LEFT' + 'RIGHT')` via `tests/e2e/helpers.py::shell_cmd`.

Rule: never paste bash quoting into PowerShell. WHY: Windows dev Tabs run
PowerShell; use `shell_cmd(unix, win)` for cross-platform command strings.
Example: `tests/e2e/test_inject_verify.py`.

Rule: assertions must not be derived from the same variable that built the
input unless the system under test transforms or transports it. WHY: fixture
tautologies go green without proving behavior. Example: transcript fixtures in
`packages/chat-ui/src/providerTranscriptFixtures.testdata.ts` are asserted via
rendered structure in `MessageBubble.providers.test.tsx`.

Rule: prove payload integrity with length or hash, not marker presence. WHY: a
truncated paste can still echo the marker. Example:
`tests/e2e/test_inject_verify.py` echoes payload length proofs for 5k payloads.

Rule: negative assertions poll absence before triggering presence. WHY: a stale
screen match is not proof. Example: `tests/e2e/test_pty_io.py` enter=false cases
poll absence, then send raw Enter and require presence.

Rule: missing preconditions must fail the test. WHY: silent pass hides broken
setup. Example: hierarchy tests now fail when the expected worker Tab is absent
instead of skipping aggregation checks.

Rule: prefer exact equality for contracts. WHY: presence-only asserts accept
wrong schemas. Example: `tests/e2e/test_agents.py` asserts concrete
`role`/`agent_type`/`session` fields for status responses.

Rule: include one real negative path when the contract has rejection behavior.
WHY: happy paths do not prove gates. Example:
`src-tauri/src/bridge/manager/tests.rs` rejects unsupported bridge services.

## Harness rules

Rule: headless e2e Tabs should be eager. WHY: lazy Tabs have no PTY until a UI
mounts them. Example: `tests/e2e/conftest.py::fresh_tab` and
`tests/e2e/test_eager_tab.py`.

Rule: do not trust inherited env. WHY: tests must target dev 7686, not prod
7685 or a user shell. Example: `tests/e2e/conftest.py` force-sets the dev API
environment.

Rule: sweep before and after daemon e2e. WHY: leaked workspaces make later tests
order-dependent. Example: `tests/e2e/conftest.py::_cleanup_e2e_workspaces`.

Rule: teardown in `try/finally`. WHY: failures must not leak workspaces, Tabs,
or external stubs. Example: `tests/e2e/test_eager_tab.py`.

Rule: keep shared inputs under `tests/fixtures/` unless a package has a local
fixture convention. WHY: review can see what is data versus test logic. Example:
`tests/fixtures/auto-update/README.md`.

Rule: no new `console.*` in QuickJS provider plugin code. WHY: plugin stdout is
not a stable contract and can mask parser failures; assert outcomes in
`plugins/*/tests/*.test.ts` instead.

Rule: LLM tests must be visibly marked. WHY: token-spending tests need explicit
human intent. Example: `pytestmark = pytest.mark.llm` in
`tests/e2e/test_claude_spawn.py`.

Rule: use the dev-locked harness for benches/evals. WHY: benchmarks must not
mutate a user's production daemon. Example: `evals/_lib/lib.sh`.

Rule: `.sh` scripts from PowerShell are always run through bash. WHY: bare
`.sh` opens a Windows "choose an app" dialog. Example:
`bash scripts/plugin-tests.sh`.

Rule: register what an inventory test watches in the same change. WHY: these
tests fail on anything unlisted. A new `IS_WEB` / `isTauriEnv` /
`__TAURI_INTERNALS__` branch in `src/` needs a classified entry in
`src/lib/webForks.test.ts`; a new prompt file needs the steps in
`.agents/skills/prompts/SKILL.md`; a new `prod-docs/<NAME>.md` is registered in the
`codeterm docs` registry. Stable product behavior belongs in `essences/`; executable procedures belong in `.agents/skills/`.

## Recipes

### Component/unit with tsx and happy-dom

1. Put the test beside the component or package under test.
2. Use the existing tsx runner pattern; do not add Playwright for pure render
   assertions.
3. Mock browser-only renderers such as mermaid.
4. Assert semantic structure, not pixels.
5. Prove fail-first by temporarily breaking a selector, fixture field, or
   expected count before the PR.

WHY: this catches renderer regressions in seconds. Example:
`packages/chat-ui/src/CodeBlock.test.tsx`,

`packages/chat-ui/src/MessageBubble.providers.test.tsx`.

### Plugin suite

1. Add or extend `plugins/<provider>/tests/*.test.ts`.
2. Use real provider fixture shapes; do not invent fields.
3. Include malformed-entry rejection only for genuinely malformed entries.
4. If Rust native parity exists, keep JS acceptance aligned with parity
   fixtures.
5. Prove fail-first by corrupting one fixture entry or expected count.

WHY: provider plugins are a boundary; the contract is accepted transcript shape.
Example: `plugins/provider-claude/tests/plugin.test.ts` and
`plugins/provider-codex/tests/provider-helper-parity.test.ts`.

### API e2e with pytest

1. Use `tests/e2e/helpers.py::CodeTermAPI`.
2. Create tabs with `eager=True` for headless PTY tests.
3. Use `shell_cmd(unix, win)` for commands that enter a shell.
4. Use split markers plus length/hash proofs for payload transport.
5. Wrap created workspaces/resources in `try/finally`.
6. Run `python -m pytest tests/e2e/<file>.py -m "not llm"` unless the test is
   explicitly marked `llm`.
7. Prove fail-first with a sabotage marker, fake length, or rejection input.

WHY: API e2e should prove daemon-visible behavior without relying on UI focus.
Example: `tests/e2e/test_inject_verify.py` and `tests/e2e/test_eager_tab.py`.

### Playwright smoke

The Playwright suite is suspended. Do not run, repair, migrate, or add Playwright
tests unless Andrey explicitly requests that run in the current conversation.
The archived suite command remains `pnpm test:pw:smoke`; keep existing specs and
manual commands intact. Use focused unit or API coverage and manual dev
inspection for UI changes.
### Golden/LLM

1. Mark real-token tests `@golden` or `pytest.mark.llm`.
2. State the budget and why lower layers cannot prove the behavior.
3. Assert structural outcomes, not prose exact text.
4. Avoid mega-tests; one golden should prove one user-critical journey.
5. Prove fail-first with a controlled prompt/task change where practical.

WHY: model output varies and costs money; use it only for live-agent confidence.
Example: `tests/playwright/suites/03-agents/agents.spec.ts`.

## Review checklist for new tests

- Lowest viable layer chosen. WHY: faster tests fail closer to the bug.
- Fail-first evidence captured. WHY: a test that never went red may be inert.
- Marker cannot match typed echo. WHY: PTYs echo input before execution.
- Payload length/hash is checked for large transfers. WHY: markers survive
  truncation.
- Negative paths wait for absence. WHY: stale output is common in terminal
  buffers.
- Preconditions fail loudly. WHY: silent setup skips are false greens.
- Teardown is in `finally` or fixture cleanup. WHY: shared dev survives test
  failure.
- LLM/golden markers are explicit. WHY: reviewers can control token spend.
