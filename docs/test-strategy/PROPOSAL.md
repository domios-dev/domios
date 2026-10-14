# Test strategy proposal

Base: develop `4d81f892d` (contains `ddfca8ee7`). Plan `pl_wuzkps`, task `task_ac7h0v`.
Companion files: [`catalogue.schema.json`](catalogue.schema.json) and
[`catalogue.draft.json`](catalogue.draft.json) (111 runner entries, 8 stale and
1 flaky overlay markers; validates with
`npx --yes ajv-cli@5 validate --spec=draft2020 -s docs/test-strategy/catalogue.schema.json -d docs/test-strategy/catalogue.draft.json`).

The structure is decided (D1-D10). This document inventories the tree, attaches
evidence to each decision, and ends with the questions only the owner can answer.

## How to read the evidence

- **Source fact**: a file and line at `4d81f892d`, or a git commit.
- **CI log**: a GitHub Actions run, read with `gh run view <id> --json jobs`.
  Job durations are `completedAt - startedAt`; run wall time is
  `updatedAt - createdAt` and includes runner queueing.
- **Measured here**: run in this lane on Windows 11 (Node v22.14.0, Python 3.12.7,
  `core.autocrlf=true`, no `node_modules`). Nothing here compiled Rust, ran
  Playwright or spawned an agent.
- **Unmeasured**: no log exists and measuring would need a Rust build, a running
  instance or a live agent. Never filled with a guess.

Reference runs:

| Run | Event | What it shows |
|---|---|---|
| `38525117769` | pull_request, `release/1.14.7` | Current PR shape, success, wall 3m58s (06:55:45Z to 06:59:43Z). |
| `38548176704` | release.yml call (`phase` fast + long) | Rust archives, clippy; shards and platform jobs cancelled after about 900s. |
| `38552758851` | release.yml call | Rust archives 479-597s; shards and platform cancelled at about 481s. |
| `38303744776` | release.yml call, success | Pre-shard per-OS Rust jobs (770s macOS, 1171s Linux, 2335s Windows). |
| `38215187464` | schedule, success | Last green nightly (older job shape); per-plugin `check-plugin` 17-26s. |
| `38620662032`, `38622431539` | pull_request | `script-catalog.yml` job 30s and 31s. |

No completed run of the current 12-shard layout (`rust-shards`, `rust-platform`,
`rust-input-platform`) was found among the sampled runs, so their durations stay
unmeasured.

## The schedule trigger: what is true

- **Source fact**: `.github/workflows/tests.yml` at `4d81f892d` has exactly four
  triggers: `workflow_call` (line 8), `pull_request` (29), `push` to `master`
  with `paths-ignore` (30-36) and `workflow_dispatch` (37). **There is no
  `schedule` trigger.**
- **Source fact**: commit `c5790bd50e` ("ci: drop the nightly scheduled test
  matrix", 2026-10-14 08:58 +0300) deleted `schedule: - cron: '0 1 * * *'` and
  nothing else. It is an ancestor of `4d81f892d`, `origin/develop` and
  `origin/master`.
- **CI log**: the last scheduled `tests.yml` run is `38518614106`
  (2026-10-14T05:48:23Z, cancelled). Of the ten scheduled runs from 2026-10-05 to
  2026-10-14, three failed (`37272972943`, `37583768199`, `38362096456`) and one
  was cancelled.
- The leftover text is therefore **false**:
  - `tests.yml:38-39` "Nightly full matrix on develop while the farm is idle ...";
  - `tests.yml:48` "The nightly schedule shares master's group.";
  - `.agents/skills/github-release/SKILL.md:21` "The nightly schedule runs the full matrix on `develop`.";
  - `scripts/ci-scope.mjs:60` "the release run and the nightly own the heavy Rust and CLI matrix";
  - `.agents/skills/github-release/SKILL.md:259` "standalone PR/nightly tests".
- 19 expressions in `tests.yml` still test `github.event_name == 'schedule'`
  (for example lines 92, 96, 123). They are dead code now.
- The only `schedule` triggers left in `.github/workflows/` are `dep-audit.yml:5`
  (weekly `pnpm audit`) and `release-credentials.yml:9`. Neither runs tests.

## Inventory

Columns: **Where**, **Command**, **Duration** (with source), **Inst** =
needs_instance, **Live** = needs_live_agent, **CI today**.

### TypeScript

| Runner | Where | Command | Duration | Inst | Live | CI today |
|---|---|---|---|---|---|---|
| TS unit | `src/**`, `packages/**` `*.test.ts(x)` minus the vitest-owned list; discovery in `scripts/run-ts-unit-tests.mjs:63-91` | `pnpm test:unit` (= `node scripts/run-ts-unit-tests.mjs`) | 124s job, run `38525117769`; 711 files (measured: `discoverTests()` here) | no | no | `tests.yml` `ts-unit` (PR when frontend changed; push; dispatch; call) |
| TS vitest | `scripts/vitest-owned-tests.json` (214 files), read by `vitest.config.ts:6-8` | `pnpm test:vitest --maxWorkers=4` | 70s job, run `38525117769` | no | no | `tests.yml` `ts-vitest` |
| TS static guards | repo | `pnpm exec tsc --noEmit`; `pnpm check:unused-exports`; `pnpm knip:report`; `pnpm check:api-effects`; `pnpm lint` | 52s job (all five), run `38525117769` | no | no | `tests.yml` `ts-static` |
| Generated types | `gen/`, `packages/*/src/gen/` | `pnpm types:check` | 34s job, run `38525117769` (warm Rust) | no | no | `tests.yml` `ts-types` (frontend or Rust changed) |
| Perf benches | `tests/perf/*.bench.ts` | `npx tsx tests/perf/transcript-churn.bench.ts`; `npx tsx tests/perf/pty-write-slicing.bench.ts` | unmeasured (no `node_modules` in lane) | no | no | none; on demand (`tests/perf/README.md:3-7`) |

### Node and Python script tests

37 Node files (`scripts/tests/*.test.mjs` 28, `scripts/*.test.mjs` 9), 5 Python
files and 1 shell file in `scripts/tests/`. `scripts/tests/workflow-condition.mjs`
is a helper, not a test.

Every place that hand-lists them today:

| Hand list | Files named |
|---|---|
| `tests.yml:156` (`plugin-metadata`) | `plugin-releases`, `ci-scope` |
| `tests.yml:818-834` (`release-contracts`) | `release-test-gate`, `ci-scope`, `release-job-graph`, `release-bundle-smoke`, `release-asset-scope`, `ci-rust-shards`, `ci-runner-paths`, `ci-rust-archives`, `updater-manifest`, `release-closure-authority`, `release-notes-sync`; Python `test_release_issue_contract`, `test_release_workflow_contract` |
| `build.sh:2388` (`script-guards`, Linux only) | `scripts/check-analytics-catalog.test.mjs` |
| `build.sh:2167-2173` (`release-preflight`) | Python `test_release_issue_contract`, `test_release_workflow_contract`; `updater-manifest`, `release-closure-authority`, `release-notes-sync`, `bundled-cli-name`, `local-install-cache` |
| `build.sh:2190` (`test-one app build_identity_required`) | `build_identity_required` |
| `file-sizes.yml:44` | `check-provider-boundary` |
| `plugin-release.yml:47` | `plugin-releases`, `ci-scope` |
| `scripts/check-build-gateway-policy.sh:56` (via `build-gateway.yml:70`) | `scripts/cache-retention.test.mjs` |

**Not named by any gate (20 Node, 3 Python, 1 shell):** `chat-scroll-criteria`,
`check-file-sizes`, `collect-rust-warnings`, `dev-frontend-ready`,
`dev-sidecar-ownership`, `dev-static-api`, `dev-static-server`, `dev-wait`,
`reap-dev-sidecars`, `release-input-reuse`, `sccache-server-probe`,
`utility-shell-reports` (all `scripts/tests/`), and `scripts/affected-tests`,
`build-gateway-evidence`, `build-gateway-mutex`, `build-history`,
`check-slow-unit-tests`, `run-ts-unit-tests`, `runner-toggle` (`scripts/*.test.mjs`);
Python `release_signing_material_test.py`, `test_convert_legacy_parked_tasks.py`,
`test_pdb_identity.py`; shell `qa-report-early-alerts.sh`.
`scripts/runner-toggle.test.mjs:1` claims "Caller: build.sh script-guards (node --test)",
but `build.sh:2377-2392` does not run it.

| Runner | Command | Duration | Inst | Live | CI today |
|---|---|---|---|---|---|
| CI hand list (12 files) | `node --test` over the `tests.yml:156` + `818-834` files | measured here: 108 tests, 1.28s, 2s wall; 6 failures, all CRLF (see below) | no | no | `release-contracts` 33s and `plugin-metadata` 11s jobs, run `38525117769` |
| Whole Node glob | `node --test scripts/tests/*.test.mjs scripts/*.test.mjs` | measured here: killed at the 600s cap (exit 124). Per file: 35 files sum to about 60s; `scripts/build-gateway-mutex.test.mjs` and `scripts/run-ts-unit-tests.test.mjs` each hit a 180s cap. **Linux unmeasured.** | no | no | partly, via the hand lists above |
| Python | `python3 -m unittest scripts.tests.test_release_issue_contract` and `...test_release_workflow_contract` | inside the 33s `release-contracts` job | no | no | `release-contracts`; `release-preflight` |
| Frontend release producer | `node --test scripts/frontend-release/*.test.mjs` (1 file) | unmeasured | no | no | `frontend-release.yml:92`, path-filtered PR, call, dispatch |

Failures seen only in this Windows lane (hypothesis for each, not proven on Linux):
`release-test-gate`, `release-job-graph`, `ci-scope` fail "Missing condition for
ts-static/fmt/rust-archives" because the checkout has CRLF line endings
(`core.autocrlf=true`; the same files pass on Linux in run `38525117769`);
`dev-sidecar-ownership`, `sccache-server-probe`, `utility-shell-reports`,
`collect-rust-warnings` and `check-analytics-catalog` fail on Unix-only paths or
tools. The two 180s files spawn real processes ("20 rounds of six real competing
Bash owners"; "real width-20 topology", which needs `node_modules`).

### Rust

Discovery today: CI archives each manifest once per OS with
`cargo nextest archive` and lists it with `cargo nextest list --message-format json`
(`scripts/ci-rust-archives.mjs:96-101`). The `src-tauri` lane uses
`--workspace --lib --exclude codeterm-input-macos --exclude codeterm-input-windows`,
the `cli` lane `--workspace` (`scripts/ci-rust-archives.mjs:67-70`).
`scripts/ci-rust-shards.mjs verify` proves every shared test ran exactly once over
12 macOS ARM64 shards plus three native platform jobs.

CI placement for every row below: `tests.yml` `rust-archives` then `rust-shards`
(12) and `rust-platform` (3 OS), gated by `heavy`, which `scripts/ci-scope.mjs:61-64`
sets to false on `pull_request`. So **no Rust test runs on a pull request**; they
run on push to `master`, `workflow_dispatch` and `workflow_call` (release).
Durations: archive (compile) 479-597s per OS in run `38552758851`; shard and
platform test time unmeasured (cancelled in `38548176704` and `38552758851`).

Static counts are `grep` hits for `#[test]`/`#[tokio::test]` attributes, an
estimate until `scripts/rust-test-inventory.mjs` (D4) exists.

| Package | Manifest / dir | `./build.sh nextest` scope | `./build.sh test-one` scope | In `./build.sh test` | Static attrs |
|---|---|---|---|---|---|
| codeterm (app lib) | `src-tauri` | `app` | `app` | yes | 6614 |
| codeterm (integration `tests/main.rs`) | `src-tauri/tests` | none (`--lib`) | none | no | 15 |
| codeterm-account | `src-tauri/crates/` | `account` | `account` | yes | 18 |
| codeterm-billing | ″ | `billing` | `billing` | yes | 8 |
| codeterm-analytics | ″ | `analytics` | `analytics` | no | 23 |
| codeterm-frontend-bundle | ″ | `frontend-bundle` | `frontend-bundle` | yes | 19 |
| codeterm-types | ″ | `types` (all targets) | `types` | yes | 190 |
| codeterm-wire | ″ | `wire` | `wire` | yes | 29 |
| codeterm-sentinel | ″ | `sentinel` (+`test-support`) | `sentinel` | yes | 125 |
| codeterm-sentinel-ports | ″ | fallback `codeterm-sentinel-ports` | none | no | 0 |
| codeterm-decision | ″ | `decision` | `decision` | yes | 4 |
| codeterm-provider-signals | ″ | `provider-signals` | `provider-signals` | yes | 5 |
| codeterm-util | ″ | `util` | `util` | yes | 72 |
| codeterm-browser | ″ | `browser` | `browser` | no | 89 |
| codeterm-mesh | ″ | `mesh` | `mesh` | yes | 152 |
| codeterm-mem | ″ | `mem` | `mem` | yes | 446 |
| codeterm-plan | ″ | `plan` | `plan` | yes | 788 |
| codeterm-bridge | ″ | fallback `codeterm-bridge` | none | yes | 103 |
| codeterm-pty | ″ | `pty` | `pty` | yes | 590 |
| codeterm-plugin | ″ | `plugin` | `plugin` | yes | 1202 |
| codeterm-plugins | ″ | `plugins` | `plugins` | yes | 96 |
| codeterm-storage | ″ | fallback `codeterm-storage` | `storage` | no | 29 |
| codeterm-agent-detect | ″ | `agent-detect` | `agent-detect` | no | 198 |
| codeterm-notification-core | ″ | `notification-core` | `notification-core` | no | 94 |
| codeterm-repowatch | ″ | `repowatch` | `repowatch` | no | 11 |
| codeterm-usage | ″ | `usage` | `usage` | no | 45 |
| codeterm-blueprint-types | ″ | fallback | `blueprint-types` | no | 27 |
| codeterm-local-recording | ″ | fallback | `local-recording` | no | 2 |
| codeterm-notify-types | ″ | fallback | none | no | 0 |
| codeterm-events | ″ | fallback | none | no | 18 |
| codeterm-config | ″ | fallback | none | no | 40 |
| codeterm-secrets | ″ | fallback | none | no | 51 |
| codeterm-keepawake | ″ | fallback | none | no | 13 |
| codeterm-cron | ″ | fallback | none | no | 21 |
| codeterm-shutdown | ″ | fallback | none | no | 3 |
| codeterm-cli | `cli/Cargo.toml` | `cli` (all targets) | `cli` | separate `cli` call | 1423 |

Platform crates, listed separately (excluded from the shared archive):

| Package | Command | Duration | CI today |
|---|---|---|---|
| codeterm-input-macos (19 attrs), codeterm-input-windows (21 attrs) | `./build.sh nextest input-platform` (`build.sh:365`: `--lib -p codeterm-input-macos -p codeterm-input-windows`) | unmeasured; job `timeout-minutes: 15` (`tests.yml:394`); runs in `38610190303` and `38620650594` were cancelled at 28-79s | `tests.yml` `rust-input-platform` on Linux, macOS, Windows (heavy only); required by `rust-coverage` (`tests.yml:500`) |

Rust test code outside the workspace that **no gate runs**:

| Package | Where | Static attrs | Evidence |
|---|---|---|---|
| codeterm-mem-store | `crates/mem-store` | 38 | path dependency (`src-tauri/Cargo.toml`, `codeterm-mem-store = { path = "../crates/mem-store" }`), not in `members`; only `dep-audit.yml:13` touches it |
| codeterm-paths | `crates/codeterm-paths` | 7 | path dependency, not in `members`; `qualification.yml:1700-1701` lists its sources for hashing only |
| portable-pty (pty-fork) | `src-tauri/pty-fork` | 16 | `exclude = ["pty-fork", ...]` (`src-tauri/Cargo.toml:9`); reachable by `./build.sh test-one pty-fork` / `nextest pty-fork` only |
| codeterm (integration) | `src-tauri/tests/main.rs` (`[[test]] name = "main"`) | 15 | CI archives `--lib`; `build.sh test` and `verify` run `--lib` |

Build verbs:

| Verb | What it runs | Where |
|---|---|---|
| `./build.sh test-one <scope> <filter>` | one package's tests through `scripts/test-capped.sh`, 600s default cap | `build.sh:2183-2257` |
| `./build.sh nextest <scope> [filterset]` | nextest for one scope; unknown `codeterm-*` falls back to `--lib` | `build.sh:351-473`, `2258-2260` |
| `./build.sh test` | `run_workspace_lib_tests`: 17 of the 36 workspace members, `--lib`, 3600s cap each | `build.sh:343-349`, `2176-2182` |
| `./build.sh verify` | `cargo check`, `run_workspace_lib_tests`, CLI tests, `tsc`, file sizes, provider boundary, empty-array expansion, script catalogue | `build.sh:2261-2275` |
| `./build.sh check-tests` | compiles all test targets of both manifests, runs nothing | `build.sh:2119-2126` |
| `./build.sh check-queue [sha]` | warm per-machine incremental check of changed Rust targets, now with clippy (`build.sh:1475-1498`) | `build.sh:1433`, `2098-2100` |
| `./build.sh release-preflight` | fmt, guards, check, clippy, `run_workspace_lib_tests`, CLI tests, TS unit, vitest, plugin tests, its own Node/Python hand list | `build.sh:2141-2175` |

`run_workspace_lib_tests` skips 19 members: both input crates, analytics,
sentinel-ports, browser, storage, agent-detect, notification-core, repowatch,
usage, blueprint-types, local-recording, notify-types, events, config, secrets,
keepawake, cron, shutdown (704 static attributes). CI nextest covers them;
`test`, `verify` and `release-preflight` do not. D10 closes this gap.

`qualification.yml` hand-listed Rust test names (102 quoted names, all
`workflow_dispatch` only, `qualification.yml:3-7`):

| Job | Selector command | Names |
|---|---|---|
| `sentinel-linux-task-local` | `./build.sh test-one app sentinel::probe::tests::` and `sentinel_probe_native_` | prefixes at 843-861, 913-918, 1367-1381 (11 portable suffixes), 1486, 1556 |
| `sentinel-command-linux` | `./build.sh test-one app sentinel_command_native_` | 1122, 1135 |
| `storage-foundation-linux` | `TEST_CAP_SECS=600 RUST_TEST_THREADS=1 ./build.sh test-one app storage_contract_` | 23 names, 1851-1873 |
| `storage-domains-linux` | `test-one app storage_cron_`, `storage_sentinel_`, `storage_contract_` | 28 + 17 + 23 names, 2439-2520 |
| `jev-linux` | `scripts/test-capped.sh --exec "$JEV_QA_PY" -m unittest discover -s evals/tab-state-jev` | Python, 77-150 |

These lists are exact expected-result oracles for one-off qualifications, not
membership lists; the catalogue records the workflow as one manual `contract`
runner and leaves the names in place.

### Instance-backed tests

| Runner | Where | Command | Duration | Inst | Live | CI today |
|---|---|---|---|---|---|---|
| API e2e | `tests/e2e` (26 `test_*.py` files, 119 `test_` functions; `pytest.ini` defines the `llm` marker) | `./build.sh e2e` (`pytest -m "not llm"`, `build.sh:2755-2761`) | unmeasured (needs DEV 7686) | yes | no | none |
| API e2e, LLM | `tests/e2e` (3 files use the marker) | `./build.sh e2e:llm` (`build.sh:2762-2767`) | unmeasured | yes | yes | none |
| `test:all` | aggregate | `./build.sh test:all`: Rust lib, api-effects, TS unit, vitest, plugins, e2e if DEV healthy | unmeasured | optional | no | none |
| Playwright | `tests/playwright` (`playwright.config.ts`, `suites/`) | `pnpm test:pw:smoke` (+ `golden`, `perf`, `stub`, `full`, `live-pair`) | not run (suspended) | yes | yes (golden, live-pair) | none |
| Manual helpers | `tests/manual/test_helpers.sh` | `source tests/manual/test_helpers.sh` | n/a | yes | no | none; referenced by no file |
| Hand tests | `scripts/hand-tests/` | `node scripts/hand-tests/terminal-native-resize.mjs`; `node scripts/hand-tests/agent-spawn-latency.mjs` | unmeasured | yes | yes (spawn latency) | none |

Playwright status: `.agents/skills/testing/SKILL.md:24-27` says the suite "is
suspended and is not part of the test plan, agent workflow, local aggregate, QA
registry, or CI"; `:612-615` repeats it and keeps `pnpm test:pw:smoke` as the
archived command. `tests/playwright/README.md`, `QUARANTINE.md` and
`COVERAGE_MAP.md` never mention the suspension; the README documents the run
commands as current and `QUARANTINE.md` documents an active quarantine policy.
`COVERAGE_MAP.md` is generated (`node scripts/playwright-coverage-map.mjs`).

`tests/fixtures` (data, not a runner) and its consumers:

| Fixture | Consumers |
|---|---|
| `agent-runtime/` | `src-tauri/src/agent/run_watcher/runtime_runs/tests.rs`, `service/agent_runtime/{controls,process}/tests.rs` |
| `agent-state/` | `evals/agent-state` |
| `auto-update/` | `tests/e2e/test_update_resume.py` |
| `blueprint-nodes.golden.json` | `codeterm-blueprint-types/src/blueprint_catalog.rs`, `src/components/Automation/blueprintKinds.test.ts` |
| `blueprint-pipeline.golden.json` | `service/sentinel/graph/pipeline/tests.rs`, `blueprintContract.testdata.ts` |
| `opencode-1.18.35-tool-parts.json`, `opencode-file-diffs.json` | `plugins/provider-opencode/tests/file-diffs.test.ts`, `codeterm-plugin` parity test, `opencode_sse/tests.rs` |
| `plan-state/` | `scripts/tests/test_convert_legacy_parked_tasks.py` |
| `pty-delivery/` | `chat_send_evidence/tests.rs`, `pty_io/send/tests.rs` |
| `rust-warnings/` | `scripts/tests/collect-rust-warnings.test.mjs` |
| `tab-respond/` | `service/tab/respond_screen/tests.rs` |
| `update-availability/` | `api/updater_routes/availability_tests.rs`, `src/lib/updateAvailability.test.ts` |
| `blueprint_terminal_harness.py`, `opencode-1.18.35-tool-parts.md` | none (`git grep` finds no reference) |

### evals

`evals/core-journeys` (D5, e2e-curated, README:1 "release blocker"):

| Piece | Role |
|---|---|
| `run.sh` | modes `all`, `setup`, `verify`, `cleanup`, `upgrade` (J5); `--dc` routes through `evals/_lib/lib.sh` |
| `driver.mjs` | runs J1-J4 (`journeys/spawn.mjs`, `team.mjs`, `dispatch.mjs`, `restore.mjs`, `cleanup.mjs`) |
| `restart.mjs` | bench-owned DEV restart, reusing `evals/restart-endurance/restart.mjs` |
| `lib/` | `cdp`, `oracle`, `results`, `wait`, `tabs`, `fixtures`, `listener`, `env` |
| README journeys table | J1 provider spawn/reply/stop, J2 Team dialog over CDP, J3 dispatch/bounce/accept, J4 restart restore, J5 upgrade N-1 to N (README:46-54) |
| README regressions table | five BLACKSTAR regressions mapped to the check that must fail on the pre-fix build (README:59-71) |

| Runner | Command | Duration | Inst | Live | CI today |
|---|---|---|---|---|---|
| core-journeys live | `bash evals/core-journeys/run.sh all` | budget "about 15 minutes" (README:28-29); unmeasured here | yes | yes | none; canonical checkout under Orchestrator QA authority (README:13-16) |
| core-journeys offline | `node --test evals/core-journeys/*.test.mjs evals/core-journeys/lib/*.test.mjs` | measured here: 25 tests, 241ms; 1 failure (`driver.test.mjs:24`, expects `/tmp/old`, gets `C:\tmp\old` on Windows) | no | no | none |

Other benches (all manual; `evals/**` is in `tests.yml` push `paths-ignore`;
`scripts/check-evals-list.sh` in `docs-source-of-truth.yml` only checks the list).
Inst and Live come from a static grep of each bench (`_lib/lib.sh`, `dc`, 7686;
agent-spawn and provider-token wording) and are confirmed per bench by the
catalogue task:

| Bench (entry) | Live per grep |
|---|---|
| agent-state, binding-matrix, browser-discovery, chat-roundtrip, codex-directory-trust, delivery-confirmation, lifecycle-recovery, long-paste-submit, manager-bootstrap, orchestrator-creation-matrix, prompt-matrix, restart-endurance, restore-lifecycle, restore-matrix, runtime-provider-matrix, session-restore, upgrade-restore, watcher-health (`run.sh`); mesh-webui (`run.mjs`); tab-state-jev (`run.py`) | yes |
| api-liveness-mesh-load, dreamer-repair, loop-deviation, mem-recall, mesh-two-dev, orchestration-golden, plan-authorized-reporting, plan-lifecycle, plugins, provider-setup-lifecycle, pty-paste-trace, session-rename-durability, web-mirror-switch-sync (`run.sh`); chat-resume-latency, heartbeat-replay, mem-rerank-live, prompt-contract (`run.py`) | no |
| blueprint-event, blueprint-input, blueprint-pipeline (`live.sh`); cli-agent (`batch.sh`); codex-remote-spike (`launch.mjs`); frontend-swap (`capture.sh`); general-agent (`prepare-prompts.py`); opencode-decision (`latency.mjs`); orchestrator-refresh (`drill.sh`); seeds (`build.sh`) | entry is not a `run.*`; no catalogue entry until the owning bench README names its runner |
| blueprint-watch, cartographer-map, dreaming, heuristics, orchestrator-control-loop, perf, routed-reply, spawned-agent-primer, worker-handoff | no runner script (docs or spikes) |

`evals/_lib`: `lib.sh` (dev-locked `dc`, `dev_run`, `dev_get`, `dev_post`),
`bench.sh`, `role_preset.sh`, and self-tests `test-dc-cli-resolution.sh`,
`test-source-purity.sh`, `test_bench_infrastructure.py`, which no gate runs.
Offline bench self-tests also exist in mesh-webui (15), restart-endurance (9),
tab-state-jev (6, run by `qualification.yml` `jev-linux`), mesh-two-dev (6) and others.

### Plugin tests

| Runner | Where | Command | Duration | CI today |
|---|---|---|---|---|
| Plugin TS tests | `plugins/**/*.test.ts` (76 files) | `pnpm test:plugins` = `scripts/run-plugin-tests.mjs` -> `scripts/plugin-tests.sh` | `ts-plugins` job 41s incl. bundles, run `38525117769` | `tests.yml` `ts-plugins` (gated on `rust`, `tests.yml:755`) |
| Plugin bundles | `plugins/*` | `pnpm check:plugin-bundles` | inside the 41s | `ts-plugins` |
| Per-plugin check | `plugins/channel.json` | `node scripts/check-plugin.mjs <id>` | 17-26s per plugin, run `38215187464` | `tests.yml` `plugins` matrix, only when no frontend/Rust change (`scripts/ci-scope.mjs:57`) |
| Plugin release metadata | `plugins/channel.json` | `node scripts/release-plugin.mjs check` | `plugin-metadata` 11s, run `38525117769` | `tests.yml` `plugin-metadata`; `plugin-release.yml` |

`scripts/plugin-tests.sh:11-81` hand-lists 62 files; 14 discovered files are not
in it (for example `plugins/analytics/tests/plugin.test.ts`,
`plugins/browser/tests/plugin.test.ts`, seven `plugins/provider-codex/tests/*`,
`plugins/provider-shared/updateCheck.test.ts`). `scripts/test-channels` does not
exist: `scripts/catalog-removed.md:26` records its removal ("0 hits").

### Guards

| Guard | Command | Duration | CI today |
|---|---|---|---|
| fmt | `cargo fmt --manifest-path src-tauri/Cargo.toml --check` and `cli/Cargo.toml` | 15s, run `38525117769` | `tests.yml` `fmt` (Rust changed; PR included) |
| clippy | `bash scripts/clippy-gate.sh` under `collect-rust-warnings.mjs` | 372s, run `38548176704` | `tests.yml` `clippy-linux` (heavy only, never PR) |
| clippy in lanes | `./build.sh check-queue` (clippy since `4d81f892d`) | warm, unmeasured | none (Worker lane gate) |
| Warnings baseline | ratchet in `scripts/collect-rust-warnings.mjs:61-73` against `scripts/rust-warnings-baseline.json` (initialized, 0 warnings) | report 17-19s, run `38548176704` | `tests.yml` `rust-warnings` -> `rust-warnings-summary.yml` |
| Script guards | `./build.sh script-guards` (`build.sh:2377-2392`): `check-script-catalog.sh`, 11 fixture scripts, 3 Linux-only checks incl. `node --test scripts/check-analytics-catalog.test.mjs` | 30s and 31s, runs `38620662032`, `38622431539` | `script-catalog.yml` (PR on `scripts/**`, `build.sh`, workflows, `package.json`, skills; push to master) |
| Script catalogue | `bash scripts/check-script-catalog.sh` with `scripts/catalog-extra.txt` (18 lines) and `scripts/catalog-removed.md` (26 lines); README via `scripts/gen-script-readme.sh --check` | inside script-guards | `script-catalog.yml`; `verify`; `release-preflight` |
| File sizes, provider boundary | `bash scripts/check-file-sizes.sh --fail`; `node scripts/check-provider-boundary.mjs` | unmeasured | `file-sizes.yml` (PR path-filtered, push) |
| Comment length | `bash scripts/check-comment-length.sh --fail --changed` | unmeasured | `comment-length.yml` |
| Docs source of truth | `check-api-docs.sh`, `check-prompts-doc.sh`, `check-evals-list.sh` | unmeasured | `docs-source-of-truth.yml` |
| CLI source of truth | `cargo test -p codeterm-cli cmd::manifest`, `... plugin_subcommands_parse`; `npx tsx packages/chat-ui/src/codeterm/recognize.test.ts` | unmeasured | `cli-source-of-truth.yml` (PR on `cli/**` etc.): **the only Rust compile on a PR today** |

### tests.yml jobs and triggers

Triggers: `workflow_call` (inputs `source_ref`, `scope`, `phase`,
`cancel_release_on_failure`), `pull_request`, `push` to `master` (ignoring
`.codeterm/**`, `.agents/skills/**`, `essences/**`, `evals/**`),
`workflow_dispatch`. No `schedule`.

`changed` computes `frontend`, `rust`, `heavy`, `plugins` with
`scripts/ci-scope.mjs`: on `pull_request` `CI_FAST_ONLY=true`, so `heavy=false`
(`tests.yml:120-126`, `ci-scope.mjs:61-64`); on `workflow_call` with
`scope=full` the full matrix is forced; push and dispatch classify `HEAD^..HEAD`,
and push skips entirely when the tree equals a parent's (`tests.yml:94-111`).

| Job | Gate | PR | push master | dispatch | call (`phase`) | PR duration, run `38525117769` |
|---|---|---|---|---|---|---|
| `changed` "tree is new" | always | yes | yes | yes | yes | 12s |
| `plugin-metadata` | `changed` success | yes | yes | yes | fast, all | 11s |
| `plugins` | plugin-only diff | yes | yes | yes | fast, all | skipped |
| `fmt` | `rust` | yes | yes | yes | fast, all | 15s |
| `rust-archives` | `heavy` | no | yes | yes | long, all | skipped |
| `rust-shards` (12) | archives success | no | yes | yes | long, all | skipped |
| `rust-input-platform` (3 OS) | `heavy` | no | yes | yes | long, all | not listed |
| `rust-platform` (3 OS) | archives success | no | yes | yes | long, all | skipped |
| `rust-coverage` | `heavy` | no | yes | yes | long, all | skipped |
| `clippy-linux` | `heavy` | no | yes | yes | fast, all | skipped |
| `ts-static` | `frontend` | yes | yes | yes | fast, all | 52s |
| `ts-types` | `frontend` or `rust` | yes | yes | yes | fast, all | 34s |
| `ts-unit` | `frontend` | yes | yes | yes | fast, all | 124s |
| `ts-vitest` | `frontend` | yes | yes | yes | fast, all | 70s |
| `ts-plugins` | `rust` | yes | yes | yes | fast, all | 41s |
| `release-contracts` | `rust` | yes | yes | yes | fast, all | 33s |
| `rust-warnings` (reusable) | always | yes | yes | yes | yes | not listed |

Recent successful PR wall times: 168s (`38518488217`), 202s (`38522093496`),
205s (`38278838629`), 211s (`38285304523`), 238s (`38525117769`), 285s
(`38295844489`), 307s (`38522431513`), 331s (`38520306202`), 371s
(`38291697365`), 391s (`38303749176`).

## Stale and flaky

| Kind | Target | Evidence | Cause | Fix task |
|---|---|---|---|---|
| stale | `picker.spec.ts:486` "@golden WORKSPACE ROW: preselected to current, help tooltip is instant" | drives `picker-workspace` (spec:488) | `ddfca8ee7` removed `data-testid="picker-workspace"` (its diff line 588); `git grep picker-workspace src` finds nothing | `task_3md5uc` |
| stale | `picker.spec.ts:497` "@golden PREVIEW LINE: reflects mode + destination, updates live on workspace change" | drives `picker-preview` (502) and `picker-workspace` (506) | `ddfca8ee7`; `picker-preview` was already removed by `fe22dea1f5` (2026-10-08, "Polish session mode cards and simplify session setup") | `task_3md5uc` |
| stale | `picker.spec.ts:514` "@golden PERSISTENCE: mode + model persist, workspace resets to current" | `picker-workspace` (523, 531) | `ddfca8ee7` | `task_3md5uc` |
| stale | `picker.spec.ts:595` "@golden CROSS-WORKSPACE: spawning into workspace B leaves NO husk in A" | `picker-workspace` (602) | `ddfca8ee7` | `task_3md5uc` |
| stale | `picker.spec.ts:669` "@golden CROSS-WORKSPACE SELECT: Start switches the active workspace to B and selects its orchestrator" | `picker-workspace` (676) | `ddfca8ee7` | `task_3md5uc` |
| stale, silent | `mobile-overflow.spec.ts:1013` "@golden mobile web primary surfaces stay inside the viewport ${vp.name}" for `320x568`, `390x844`, `430x932`: the "picker workspace dropdown" audit | `if (await workspaceSelect.isVisible()...)` at 1073-1079 skips the audit, so the test passes without auditing | `ddfca8ee7` | `task_3md5uc` |
| flaky | `sentinel::tests::sentinel_probe_bounded_runtime_and_output` (`src-tauri/src/sentinel/tests.rs:481`, binary `codeterm`) | passes alone, fails among about 208 parallel tests (`task_k0ywli` brief); not reproduced here (no Rust build in lane) | load-sensitive deadline | `task_k0ywli` (D9) |

All nine are in `catalogue.draft.json` `overlay`.

## Decisions D1-D10

**D1. A catalogue over the existing locations; two moves only.**
Tests stay where they are. Moves: `tests/manual/test_helpers.sh` (referenced by
no file; `git grep test_helpers.sh` outside `tests/manual` is empty), and the Node
script tests grouped under one discovery glob,
`scripts/tests/*.test.mjs` + `scripts/*.test.mjs`, replacing the hand lists at
`tests.yml:156`, `tests.yml:818-834` and `build.sh:2388`. The `build.sh` edit
lands after Bastion's `task_2d0bug` (spec, Phase B). Evidence that hand lists
drift: 20 of 37 Node files are named by no gate, and
`scripts/runner-toggle.test.mjs:1` claims a `script-guards` caller that does not
exist. Inventory finding to act on before the swap: see Owner decision 7.

**D2. Per-runner catalogue, membership by discovery, plus the overlay.**
`tests/catalogue.json` holds one entry per runner (draft: 111), never test names.
`tests/README.md` is generated and gated. `scripts/check-script-catalog.sh` is
linked (`script-catalog` and `script-guards` entries), never replaced.
`scripts/vitest-owned-tests.json` stays authoritative; `vitest.config.ts:6-8`
and `scripts/run-ts-unit-tests.mjs:85-91` both read it, and the catalogue checks
that `ts-unit` and `ts-vitest` partition the discovered files. Consequence:
the `plugin-tests` entry discovers 76 files where `scripts/plugin-tests.sh`
lists 62.

**D3. Rust entries per package.** One entry per workspace member and `cli`
(draft: 36 + cli), plus the platform pair. Membership comes from
`cargo nextest list --message-format json` where compiling is allowed (CI already
does this: `scripts/ci-rust-archives.mjs:101`), otherwise from the static scan.
The schema enforces `fallback: static-scan` on every `nextest-list` discovery.

**D4. `scripts/rust-test-inventory.mjs`** (task rust-oracle) is the static scan.
The counts in this document are a `grep` stand-in until it lands.

**D5. core-journeys adopted as `e2e-curated`.** Entry `core-journeys`
(`bash evals/core-journeys/run.sh all`, live, instance) plus
`core-journeys-offline`. Its README already defines journeys, an evidence
layout (`results.json`, `results.md`, `restart.json`) and a regressions table.

**D6. Playwright never on PR.** No workflow invokes Playwright today
(`git grep "test:pw\|playwright test" .github` is empty). The draft marks the
entry `status: suspended`, `tier: manual`, and the schema forbids a `pr` entry
that needs an instance or a live agent. Reconciliation of the docs is Owner
decision 1.

**D7. PR tier = today's PR + Node script tests.** Budget: 5 minutes target,
8 minutes hard cap. Measured run `38525117769` took 3m58s. The schema caps any
`pr` entry at `budget_secs <= 480`. Evidence for the risk: PR wall times
already range 168-391s (above), and the whole Node glob is unmeasured on Linux;
two of its files exceeded 180s each on Windows (Owner decision 7).

**D8. Tiers `pr | release | manual`, no nightly.** The schema enum has exactly
these three. The schedule was removed by `c5790bd50e`; the leftover text and 19
dead `event_name == 'schedule'` expressions are listed above (Owner decision 2).

**D9. The sentinel flaky fix is its own task**: `task_k0ywli`, recorded in the
overlay with `fix_task`.

**D10. `release-preflight` and the gates skill use `select --tier release`.**
Evidence: `release-preflight` keeps a third Node/Python hand list
(`build.sh:2167-2173`) and runs `run_workspace_lib_tests`, which skips 19
workspace members that CI covers; selecting by tier closes both gaps.

## Migration safety

No step may leave a current gate red:

1. Catalogue and overlay land first, read-only toward gates (`validate`, `select`).
2. Before the Node glob replaces any hand list, run it once on the Linux light
   runner and record the result; files red there are fixed or carried as overlay
   markers with a task, never silently dropped.
3. ci-tiers swaps the PR commands only with the before/after line-by-line list
   (spec, Phase C).
4. Rust membership changes (Owner decision 5) are proven green in canonical QA
   before any tier includes them.

## Owner decisions

1. **D6 skill reconciliation.** `.agents/skills/testing/SKILL.md:24-27` and
   `:612-615` call Playwright suspended; `tests/playwright/README.md`,
   `QUARANTINE.md` and `COVERAGE_MAP.md` present it as active. **Recommendation:**
   the skill stays the authority. Add a one-line "Suspended: see
   .agents/skills/testing/SKILL.md; runs only on an explicit owner request" banner
   at the top of `README.md` and `QUARANTINE.md`, emit the same line from
   `scripts/playwright-coverage-map.mjs` so the generated `COVERAGE_MAP.md`
   carries it, and mark the catalogue entry `status: suspended`. The specs and
   `pnpm test:pw:*` commands stay; the picker fix (`task_3md5uc`) is an explicit
   owner request. Approve?
2. **D8 nightly.** Add a `schedule` trigger back to `tests.yml`?
   **Recommendation: no.** BLACKSTAR is paused as a runner
   (`.codeterm/build-machine/paused/BLACKSTAR.json`, commit `2923c7b9e6`: all four
   BLACKSTAR runners, including `BLACKSTAR-wsl-ci` with `linux-ci`/`light`/`heavy`,
   and `windows-x64-build` lent to ROG); the farm comment at `tests.yml:40-43`
   says the single self-hosted Linux runner is the one a release needs; three of
   the last ten nightlies failed and one was cancelled. Release-call runs already
   run the full matrix on the exact release SHA. Also approve that ci-tiers deletes
   the 19 dead `event_name == 'schedule'` expressions along with the false comments
   (`tests.yml:38-39`, `:48`, `scripts/ci-scope.mjs:60`,
   `.agents/skills/github-release/SKILL.md:21`, `:259`)?
3. **D1 moves.** Approve both: (a) move `tests/manual/test_helpers.sh` to
   `scripts/hand-tests/test_helpers.sh`, next to the other hand tests, with a
   `Caller:` header and a mention in `scripts/hand-tests/README.md` so
   `check-script-catalog.sh` passes (or name another destination); (b) one glob,
   `scripts/tests/*.test.mjs scripts/*.test.mjs`, replaces the hand lists at
   `tests.yml:156`, `tests.yml:818-834` and `build.sh:2388` (after `task_2d0bug`).
   The workflow-specific lists stay: `file-sizes.yml:44`, `plugin-release.yml:47`,
   `frontend-release.yml:92` and `check-build-gateway-policy.sh:56` (Bastion's).
   `build.sh:2167-2173` goes through D10.
4. **Python script tests.** Bring all five `scripts/tests` Python files under one
   discovery runner (`python3 -m unittest discover -s scripts/tests -t . -p '*test*.py'`,
   PR tier), adding `release_signing_material_test.py`,
   `test_convert_legacy_parked_tasks.py` and `test_pdb_identity.py`, which no
   gate runs today? **Recommendation: yes**, after the Linux green run of
   Migration step 2.
5. **Rust test code no gate runs.** `crates/mem-store` (38 attributes),
   `crates/codeterm-paths` (7), `src-tauri/tests/main.rs` (15, excluded by
   `--lib` in `scripts/ci-rust-archives.mjs:69`) and `src-tauri/pty-fork` (16).
   **Recommendation:** release tier for the first three once canonical QA shows
   them green (mem-store and codeterm-paths are linked into the app);
   `pty-fork` stays manual as vendored code (`build.sh:2227-2231`). Approve?
6. **core-journeys at release.** Its README calls it a release blocker, but it
   needs a live DEV and spends tokens, so the schema keeps it out of the unattended
   `release` tier. **Recommendation:** tier `manual`, plus a mandatory
   "core-journeys `all` green on the release SHA" step in
   `.agents/skills/github-release/SKILL.md`, run by the Orchestrator under QA
   authority. Approve, or should a red run only warn?
7. **PR budget for slow script tests.** `scripts/build-gateway-mutex.test.mjs`
   and `scripts/run-ts-unit-tests.test.mjs` each exceeded 180s in this Windows
   lane; Linux timing is unknown. If either exceeds 60s on the Linux light
   runner, it would push the PR tier past the 5-minute target.
   **Recommendation:** the catalogue task measures the glob on Linux first; any
   file over 60s gets its own `release`-tier runner entry and is excluded from the
   PR glob by name. Approve that rule?
