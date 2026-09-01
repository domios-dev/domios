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

1. `rust-unit` — uncapped `cargo test --lib` through `scripts/test-capped.sh` (always runs)
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

# TypeScript: every src/** + packages/** unit test (direct Node scheduler)
npm run test:unit
# …or one file through the same scheduler:
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

### Rust test wrapper preflight

`scripts/test-capped.sh` is the required Rust-test wrapper. In a fresh worktree,
it creates the ignored, zero-byte `bundled-embed` sidecar only when that sidecar
is absent, then waits for the requested test to finish. Set
`TEST_CAP_SECS=<positive-seconds>` only for a targeted diagnostic that needs a
process-group deadline and exit 124. The wrapper logs the test-only repair once;
a later run and a real canonical sidecar are no-ops. The bundle script rejects
empty sidecars, so this preflight cannot produce a release artifact.

The TypeScript scheduler also waits for every file and the full suite by
default. Set `TS_UNIT_TIMEOUT_SECONDS=<positive-seconds>` or
`TS_UNIT_SUITE_TIMEOUT_SECONDS=<positive-seconds>` only for an explicit
diagnostic deadline; ordinary `npm run test:unit` has neither implicit limit.

On Linux, a repository-watcher silence test that reports existing `refs/`
directories is an inotify access event from recursive watch registration, not
repository churn. Keep access filtering upstream of the debouncer, then rerun
`repository_watcher_stays_silent_on_irrelevant_churn`; never add a settle wait.

Verify the wrapper itself with the focused shell test:

```bash
bash scripts/test-capped.test.sh
```

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

End-to-end and cross-platform verification: `codeterm docs E2E-TESTING`. Prompt detection: `codeterm docs PROMPT-DETECTION`. Test design rules: `codeterm docs TEST-AUTHORING`.

<!-- revision: 35 -->
