# Codex and OpenCode spawn and restore matrix

Refs rollacode/codeterm#557 and #555. This is a live behaviour bench, not a
source-only verdict. It restarts only `mobileqa` at port `7690`. It never
rebuilds, changes product source, rewrites provider hooks, or changes global
Codex executables. The configured Codex entry point must be an npm env-node
wrapper; a native entry point fails the GUI fixture prerequisite.

## Authorized execution

Wait for the Orchestrator to supply the exact merged `release/1.14.2` SHA and
send “restart bench is yours” with that final SHA. First check `http://127.0.0.1:7690/api/health`.
The runner checks the named instance, approved SHA and canonical checkout HEAD
before a lifecycle operation. Every CLI command runs through `dc`; the Python
observer receives its loopback URL and token only through `dev_run`.

Create a Team-bound QA utility shell in the calling agent's Team. In that shell,
run this from the bench lane, substituting the approved literal SHA:

```bash
RL_PAD_DIR=/absolute/path/from/the/calling/agents/pad
export RL_PAD_DIR
CODETERM_INSTANCE=mobileqa CODETERM_API_PORT=7690 \
RL_EXPECTED_SHA=APPROVED_LITERAL_SHA \
RL_PREBUILT_CWD=/Users/dev/Developer/codeterm \
bash evals/restore-lifecycle/run-codex.sh
```

`RL_CODEX_MODEL` can select an advertised cheap model for the three Workers.
Otherwise the runner selects the catalogue entry described as affordable.
The pair uses the provider defaults as required by the spawn scenario.
`RL_BIND_DEADLINE` defaults to 180 seconds and `RL_TURN_DEADLINE` to 240;
these are failure deadlines around events, never readiness delays.

The GUI mode uses a bench-only Node preload to constrain **only** the daemon
child launched by `build.sh dev-prebuilt`; build tooling retains Node on its
normal PATH. The app receives exactly `/usr/bin:/bin:/usr/sbin:/sbin`, HOME,
USER and named DEV routing variables. The preload is absent from the app's
environment. `ps eww` of the actual process listening on 7690 proves its PATH;
only non-secret environment fields are retained. A separate restricted-PATH
version probe must reproduce the wrapper's missing Node exit 127 while the
daemon's `agent cli status codex` detects the version.

## Required evidence

| Phase | Observable PASS condition | Evidence files in each mode directory |
|---|---|---|
| 1 GUI / normal environment | Approved build, actual daemon environment, npm wrapper provenance, detected version | `health.json`, `*-daemon-env.txt`, `codex-entrypoint.json`, `cli-status.json`, `wrapper-minimal-path.txt` (GUI) |
| 2 spawn matrix | Codex pair plus three task-bound Workers in one cwd; five distinct own sessions, matching nonces, agent_ready, startup/tool ACK and durable Worker ACK; Browser shows initialized Chat without Starting | `team-pair.json`, `dispatch-*.json`, `spawn-Gamma.json`, `spawn.json`, `task-ack-*.json`, `browser-chat-*.stdout`, `transcript-*.path` |
| 3 busy restart, three cycles | Both Workers Working at shutdown; unchanged sessions and identity; one owed continue per own transcript and per-tab delivered log; numeric original work appears after that continue; original user turns survive; reconcile has no in_progress_tab_gone | `cycle-*-before.json`, `cycle-*-after.json`, `cycle-*-restart.log`, `cycle-*-dev.log`, `cycle-*-reconcile.json` |
| 4 restore queue, three cycles | Each Worker was pre-ready and Chat send returned held at queue admission; unique marker exactly once in its own transcript, zero in others; no expired_launch/refused_pending | `cycle-*-queue-window.json`, `cycle-*-queue-*.json`, `cycle-*-dev.log`, exact bound transcript paths |
| 5 respawn and old close | New ACKed attempt remains open after old close; new Worker's real report reaches persisted in_review; no stale binding | `respawn.json`, `respawn-after-close.json`, `report-task.json`, `respawn-dev.log` |
| 6 foreign Codex | Scratch Codex exits successfully with inherited Worker identity; tool evidence proves execution; own binding nonce, attempt and session unchanged | `foreign-before-task.json`, `foreign-after-task.json`, own transcript's helper JSON tool output |
| 7 idle restart | All agents idle; same bound sessions and all user turns preserved; no new user turn or delivered continue | `idle-before.json`, `idle-after.json`, `idle-dev.log` |

Each phase emits an explicit `PASS mode=... phase=...` or `FAIL` line and
`results.md` contains the mode table. Failure preserves the fixture and exact
DEV log slice in `failure-dev.log`, plus `failure.txt` and command stderr/exit
receipts. Unexecuted dependent phases are labelled `FAIL / NOT EXECUTED`, never
reported as product failures. A missed restore queue window fails the harness;
it is not evidence that queued delivery works. Fix the causal harness issue
before running a new trial, rather than retrying blindly.

The Browser uses a fresh local mobileqa token fragment and stores redacted
receipts. A missing plugin, inaccessible page, unexpected accessible tab name
or incomplete Chat proof fails phase 2. Preserve its snapshot and correct the
harness against the observed UI; an API status cannot substitute for that proof.
The browser is closed on completion or failure. Successful runs close their
Team/workspace/plan. Failed agent fixtures remain for forensics; the calling
Worker must close every utility shell it opened before reporting.

## Focused source-only checks

Run once after related edits in a Team QA utility shell:

```bash
PYTHONPATH=evals/restore-lifecycle python3 -m unittest discover \
  -s evals/restore-lifecycle -p 'test_*.py' -v
node --test evals/restore-lifecycle/gui-env.test.cjs
bash -n evals/restore-lifecycle/run-codex.sh evals/restore-lifecycle/run.sh
```

The pure evidence tests reject duplicate/cross-bound sessions and markers,
missing readiness/ACK, duplicate or missing continues, closed new attempts,
pre-restart output masquerading as resumed work, shell PATH leakage, and prod
observer routing. The preload test inspects the actual spawned fixture child.
These checks prove the harness mechanisms; only the approved live matrix proves
Codex restores on the release build.

The same run continues with [OpenCode phases 8–11](OPENCODE.md), including
provider-default and cheap models, visible first replies and lazy-session
restore. Idle tabs owe zero continues; only interrupted Working tabs owe one.
