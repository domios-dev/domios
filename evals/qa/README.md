# Curated live QA catalogue

**Before executing a case, read [Repeatable curated QA](REPEATABLE-QA.md), then the selected `case.md`.** [Test-family navigation](../README.md) distinguishes curated journeys, deterministic tests, load/performance and agent evals.

This is a rerunnable operator catalogue, not an automatic build gate. The testing skill owns test-layer policy; each case defines the live oracle and required evidence. A case never authorizes a restart, production mutation, remote pairing, reset consumption, account action, or external message by itself.

## Catalogue workflow

`catalogue.json` is the only machine-readable case listing. Each active or retired case has one stable global `CUR-NNN` ID and one folder. IDs stay ordered and contiguous, are never reused, and remain indexed after retirement. `case.md` carries the procedure; `areas/<area>.md` links cases for human reading. Each migrated legacy recipe is preserved under its case's `assets/legacy-recipe.md`.

```sh
node evals/qa/tools/catalogue.mjs validate
node evals/qa/tools/catalogue.mjs list --tier release-core
node evals/qa/tools/catalogue.mjs list --surface phone --platform windows
node evals/qa/tools/catalogue.mjs plan 1.14.9
node evals/qa/tools/catalogue.mjs matrix 1.14.9 target-check/evals/qa/1.14.9/results.json
node --test evals/qa/tools/*.test.mjs
```

Use the wildcard test command on Windows; Node's directory selector `node --test evals/qa/tools` resolves as a module path there. The catalogue tool is local-only and never calls DEV, Browser, or production. Listing filters accept tier, surface, platform, and area. A release plan expands active `release-core` and `regression` cases; exploratory charters remain optional sessions.

Platforms and surfaces describe the target, not proof of a pass. Results belong under `target-check/evals/qa/<stamp>/<CUR-id>/` with a `results.json` envelope beside them. `results.schema.json` defines the row shape. `matrix <version>` merges supplied results, refuses duplicate target rows, and fills missing rows as `NOT RUN`. Cases never contain verdict tables. The only verdicts are `PASS`, `FAIL`, `NOT RUN`, `BLOCKED`, and `OWNER-SKIPPED`. PASS/FAIL require evidence; NOT RUN/BLOCKED require a reason; OWNER-SKIPPED notes begin `Owner decision:` and are repeated in the matrix scope-out table. NOT RUN is never converted to PASS.

## Release core

Run each required row on the release candidate under the Orchestrator's serialized QA grant. A required NOT RUN leaves that acceptance row incomplete. Sign-in remains mandatory before release. Optional integrations require an explicit owner scope-out in the release matrix.

| Case | Required slice | Extra scope needs a separate grant |
|---|---|---|
| [CUR-012 sign-in](cases/CUR-012-installed-sign-in/case.md) | Installed Firebase sign-in, same account, Account UI, and sign-out | Owner restart and protected negative-redeem fixtures |
| [CUR-001 delivery](cases/CUR-001-chat-send-once/case.md), [CUR-002 Stop](cases/CUR-002-chat-stop/case.md) | Owned Codex and Claude sessions, exact own-session turns, one visible row, Stop persistence | Heavy turns, memory pressure, provider subagents |
| [CUR-004 images](cases/CUR-004-image-attachment/case.md) | PNG/JPEG bytes reach the intended session with expected opaque fixture fact | Physical iOS/HEIC/pickers and injected upload failure |
| [CUR-009 Codex balance/reset](cases/CUR-009-codex-balance-reset-cancel/case.md) | Own balance, reset count/expiry, and Cancel with zero consume requests | Confirming a reset is never authorized |
| [CUR-010 worker reporting](cases/CUR-010-worker-reporting-handoff/case.md) | Assigned ACK/report and task-less report with explicit review handoff | Hostile sandbox probes and expanded role matrix |
| [CUR-011 restart/restore](cases/CUR-011-restart-restore-continuity/case.md) | Owner-authorized task/session continuity and no unintended thaw/replay | Busy shutdown and upgrade cycle |
| [CUR-013 Telegram](cases/CUR-013-telegram-routed-reply/case.md) | Owner-authorized real inbound and far-end reply in the intended thread | Bot/poller transfer, topic deletion, main-chat smoke |
| [CUR-014 jobs/mesh](cases/CUR-014-mesh-jobs-and-inventory/case.md) | Persisted manual/scheduled results and read-only peer inventory | Pairing, remote mutation, or schedule changes outside owned fixtures |
| [CUR-015 Codex update](cases/CUR-015-codex-update-handoff/case.md) | When updater/runtime restart changes are in release scope: installed binary, runtime migration, and exact-session handoff | Owner update grant and installed package |

## Execution and evidence rules

Read `.agents/skills/testing/SKILL.md`, `.agents/skills/dev-testing/SKILL.md`, `codeterm docs BROWSER`, and each selected case's owning docs. The repository bench library overrides stale connection advice. From Git Bash:

```bash
unset CODETERM_CLI_PATH
source evals/_lib/lib.sh
source evals/qa/tools/dev-session.sh
qa_run health dev_get /api/health
qa_run topology dc tab list
```

Set `QA_OUT` to a new run directory before sourcing the logger. Every DEV action uses `dc`, `dev_get`, `dev_post`, or `dev_run`; never inherit a host or production caller. Record an actual owned DEV identity before using it. Host tab IDs are not DEV identities. Bodies live in a pad and use `--task-file` or `--from-pad`; never interpolate them into shell arguments.

Pin build SHA, boot ID, API port, and provider/plugin provenance before action. Keep an ownership manifest for workspaces, Teams, tabs, sessions, jobs, Browser targets, processes, and files. Each result includes UTC time, exact action, raw result, IDs, far-end state, opened evidence path, verdict, scope, and cleanup. A request, ACK, HTTP 200, screen echo, or source fact alone does not prove the far end.

`PASS` requires fresh far-end proof for every selected subcase. `FAIL` records an executed mismatch. `NOT RUN` records no valid execution, including missing device or capability. `BLOCKED` records an external prerequisite that stopped a planned case. `OWNER-SKIPPED` records a release scope-out decision. A failed driver is not a verified product failure until its input and target are proven. Repairs need a new result; preserve the old failure.

Wait on own-session events, process exit, DOM mutation, animation completion, or acknowledged protocol state under a failure deadline. No sleeps, settle delays, polling intervals, blind retries, or replay of a held/written/uncertain send. A timeout is not readiness.

## Secret and production boundary

The generic tools refuse production. Sign-in is a separately authorized installed-production recipe: port 7685 is permitted only for that exact account grant, and production is never killed. Secret-store values, tokens, challenge/verifier/state/code, and email/UID stay in process memory. Never capture credentials in argv, logs, pads, screenshots, page titles, or reports. Filter URLs and duplicated page-title prefixes; report UID equality as `same_uid=true`, never the UID.

## Practices adopted

1. VS Code endgame test-plan items link verification to defects and platforms. We use stable IDs, task-key coverage, and platform rows, without copying the staffing schedule. [VS Code endgame testing](https://github.com/microsoft/vscode/wiki/Endgame-testing-and-verification)
2. Chromium and Mozilla manual cases define actions and expected outcomes. We use fixed sections, explicit PASS/FAIL/NOT RUN oracles, and evidence rules. [Chromium test cases](https://www.chromium.org/for-testers/test-cases/), [Mozilla manual testing](https://blog.mozilla.org/fxtesteng/2014/09/02/web-qa-manual-testing/)
3. Kubernetes separates a curated conformance subset from broader end-to-end tests. We use release-core/regression tiers without calling this catalogue an automated conformance suite. [Kubernetes conformance proposal](https://www.kubernetes.dev/resources/keps/5922/)
4. Playwright annotations support selection and issue context. We use structured tier/surface/platform filters and task/defect coverage, without copying framework syntax. [Playwright annotations](https://playwright.dev/docs/test-annotations)
5. Session-based testing uses a mission, time box, heuristics, and reviewable findings. Exploratory cases therefore explain how a confirmed fixed defect becomes a regression case. [Bach, Session-Based Test Management](https://www.satisfice.us/articles/sbtm.pdf)

The five practices above remain the adopted set.
