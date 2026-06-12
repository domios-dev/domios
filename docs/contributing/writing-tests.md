# Writing Tests

This is the canonical "how to write a test here" page. Use `docs/TESTING.md`
for the full layer taxonomy and runbook; use this page for test shape,
assertion discipline, and review expectations.

## Test pyramid

Prefer the lowest layer that can prove the behavior.

| Prefer | Layer | Command | WHY | Example |
|---|---|---|---|---|
| 1 | Component/unit | `bash scripts/ts-unit-tests.sh` | Fast tests catch renderer/parser regressions without booting the app. | `packages/chat-ui/src/MessageBubble.providers.test.tsx` |
| 2 | Plugin suites | `bash scripts/plugin-tests.sh` | Provider contracts run outside QuickJS/Tauri and prove fixture parity cheaply. | `plugins/provider-codex/tests/provider-helper-parity.test.ts` |
| 3 | API e2e | `python -m pytest tests/e2e/ -m "not llm"` | REST/socket/PTY contracts need the daemon but not the browser. | `tests/e2e/test_inject_verify.py` |
| 4 | Playwright smoke | `pnpm test:pw:smoke` | Use only when DOM/WebView behavior is the subject. | `tests/playwright/suites/01-smoke/smoke.spec.ts` |
| 5 | Golden/LLM | deliberate only | Real tokens and model variance make these proof-of-last-resort tests. | `tests/playwright/suites/03-agents/agents.spec.ts` |

Rule: if a component test can prove it, do not write Playwright. WHY: browser
tests are slower and flakier; `MessageBubble.providers.test.tsx` proved provider
rendering without Playwright.

Rule: plugin parser changes need plugin tests and, when native parity exists,
parity fixtures. WHY: provider JS can reject shapes Rust accepts; see
`src-tauri/src/plugin/js_runtime/parity_test.rs`.

Rule: API e2e defaults to non-LLM. WHY: provider-token tests are deliberate
spend; `tests/e2e/test_providers.py` is marked `llm`.

## Honesty rules

Rule: prove command output with split markers. WHY: a terminal echo of the typed
line can satisfy a naive substring assert. Example:
`tests/e2e/test_pty_io.py` builds `echo 'LEFT''RIGHT'`; on Windows use
`Write-Output ('LEFT' + 'RIGHT')` via `tests/e2e/helpers.py::shell_cmd`.

Rule: never paste bash quoting into PowerShell. WHY: Windows dev panes run
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
setup. Example: hierarchy tests now fail when the expected worker pane is absent
instead of skipping aggregation checks.

Rule: prefer exact equality for contracts. WHY: presence-only asserts accept
wrong schemas. Example: `tests/e2e/test_agents.py` asserts concrete
`role`/`agent_type`/`session` fields for status responses.

Rule: include one real negative path when the contract has rejection behavior.
WHY: happy paths do not prove gates. Example:
`src-tauri/src/bridge/manager/tests.rs` rejects unsupported bridge services.

## Harness rules

Rule: headless e2e panes should be eager. WHY: lazy panes have no PTY until a UI
mounts them. Example: `tests/e2e/conftest.py::fresh_pane` and
`tests/e2e/test_eager_pane.py`.

Rule: do not trust inherited env. WHY: tests must target dev 7686, not prod
7685 or a user shell. Example: `tests/e2e/conftest.py` force-sets the dev API
environment.

Rule: sweep before and after daemon e2e. WHY: leaked workspaces make later tests
order-dependent. Example: `tests/e2e/conftest.py::_cleanup_e2e_workspaces`.

Rule: teardown in `try/finally`. WHY: failures must not leak workspaces, panes,
or external stubs. Example: `tests/e2e/test_eager_pane.py`.

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
2. Create panes/tabs with `eager=True` for headless PTY tests.
3. Use `shell_cmd(unix, win)` for commands that enter a shell.
4. Use split markers plus length/hash proofs for payload transport.
5. Wrap created workspaces/resources in `try/finally`.
6. Run `python -m pytest tests/e2e/<file>.py -m "not llm"` unless the test is
   explicitly marked `llm`.
7. Prove fail-first with a sabotage marker, fake length, or rejection input.

WHY: API e2e should prove daemon-visible behavior without relying on UI focus.
Example: `tests/e2e/test_inject_verify.py` and `tests/e2e/test_eager_pane.py`.

### Playwright smoke

1. Use `tests/playwright/helpers/fixtures.ts`.
2. Tag every test `@smoke`, `@golden`, or `@quarantine`; untagged tests fail
   lint.
3. Keep smoke deterministic and LLM-free.
4. Assert UI and API agree when both are relevant.
5. Prove fail-first by changing a selector, marker, or expected API field.

WHY: smoke is the browser/WebView gate, not a general integration bucket.
Example: `tests/playwright/suites/01-smoke/smoke.spec.ts`.

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

<!-- revision: 1 -->
