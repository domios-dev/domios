# evals/orchestrator-refresh

Drills for the daemon-orchestrated **orchestrator refresh** cycle — the verified
S1..S5 state machine behind `codeterm orchestrator refresh`:

```
S1 handoff snapshot  →  S2 compact  →  S3 wait-for-settle  →  S4 re-orient  →  S5 verify resumption
```

Each step advances ONLY on verified completion of the previous. The one
forbidden outcome is silence: every branch ends in verified resumption or a
loud alert.

## What the drills prove

- **`drill.sh`** — the ordered-transcript proof. Asserts the strict order
  `s1_handoff < compact cmd < s3_settle < s4_reorient < s5_verify < resumed`
  from the dev log, then verifies the plan remains readable after resumption.
- **`drill_failure_branches.sh`** — the two must-never-strand branches:
  - **hung compact** → still reaches S4 re-orient on settle timeout;
  - **dead resumption** → exactly one re-nudge, then an ALERT (notification +
    a `refresh-stranded` heartbeat learning that the planner surfaces as a
    decision gate).
- **`token-accounting.md`** — template for the cycle-cost-vs-drift-cost numbers.

## Prerequisites

- Dev daemon healthy on 7686: `curl -s http://127.0.0.1:7686/api/health`.
- The bench lib auto-locks every `dc` call to dev — never invoke `codeterm`
  directly here (see the repo's *Bench safety* rule).

## Run

```bash
# Ordered drill — Claude orchestrator (default)
PROVIDER=claude ./evals/orchestrator-refresh/drill.sh

# Ordered drill — Codex orchestrator. NOTE: codex session usage may be
# exhausted (resets on its own cadence); the orchestrator schedules this run
# when headroom returns. The drill logic is identical — only the tab provider
# differs, and compaction is capability-keyed (no provider-id literals).
PROVIDER=codex ./evals/orchestrator-refresh/drill.sh

# Failure branches. Set the timeouts LOW first so it finishes fast:
#   [orchestration.refresh]
#   settle_timeout_secs = 5
#   verify_timeout_secs = 20
./evals/orchestrator-refresh/drill_failure_branches.sh
```

Run drills ONE at a time; each is self-contained and cleans up its own
workspaces via the shared lib on the next `reset_dev_state`.

## Capability-keyed compaction (S2)

The compact step reads the provider descriptor's `compact_command` — never a
provider-id literal:

- **Mode A** — provider declares a compact command (`/compact` for Claude,
  codex per its CLI): sent via `Messenger::deliver`.
- **Mode B** — no compact capability but a resume mechanism exists: clean tab
  restart + session resume.
- **Neither** — skip-with-log. Never pretends.

## Config

`[orchestration.refresh]` in the dev config:

| key                    | default | meaning |
|------------------------|---------|---------|
| `after_reflection`     | `false` | auto-refresh after a reflection learning (OFF until drills pass) |
| `handoff_timeout_secs` | `60`    | S1: alert if no handoff stride lands |
| `settle_timeout_secs`  | `120`   | S3: proceed to re-orient anyway on timeout |
| `reorient_timeout_secs`| `30`    | S4: proceed to verify on delivery/timeout |
| `verify_timeout_secs`  | `180`   | S5: re-nudge once, then alert |

<!-- revision: 1 -->
