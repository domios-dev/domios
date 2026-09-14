# Restore Lifecycle

Proves that a Team of agent tabs survives full dev restarts (the reboot
analogue) behind the same tab ids, with the same provider, session, role, team
and title. Dev only (port 7686); every CLI call goes through the `evals/_lib`
`dc` wrapper.

## Run

From the repo root, with dev already up (`./build.sh dev --wait`):

```bash
bash evals/restore-lifecycle/run.sh
```

Tunables (env):

- `RL_CYCLES` (`2`) — full stop/start cycles.
- `RL_MIDTURN_CYCLE` (`2`) — the cycle whose restart happens while the Claude
  Worker is mid-turn (`status == Working`). Set it outside `1..RL_CYCLES` to skip.
- `RL_CLAUDE_MODEL` (`haiku`), `RL_CODEX_MODEL` (empty = the provider's
  `default_model` from `/api/agents/providers`). A requested model the provider
  does not advertise refuses the run before anything spawns: the daemon would
  otherwise fall back to its default and the bench would test a different model.
  Each spawn's resolved `model` must contain the requested one.
- `RL_BIND_DEADLINE`, `RL_TURN_DEADLINE`, `RL_REPORT_DEADLINE` — failure
  deadlines in seconds. Waits re-check on daemon events; elapsed time is never
  success.

- `RL_OWNED_DEADLINE` (`180`) — failure deadline for the pre-restart provider
  ownership gate below.

## Outcomes: PASS, SETUP, FAIL

| Exit | Last line | Meaning |
|------|-----------|---------|
| `0` | `== restore lifecycle PASS cycles=2 midturn_cycle=2 run=…` | Every cycle restored every agent. |
| `3` | `== restore lifecycle SETUP cycle=N (no restart taken; not a restore verdict) run=…` | Before cycle N, a provider store did not own a captured session. No restart ran, so nothing about restore was measured. |
| `1` | a `FAIL …` line | Setup was proven and restore broke (report, identity, or a resumed turn), or the harness itself failed. |

Before every restart the bench waits, per agent tab, until the tab's provider
store owns the session captured in `identity-before.json`:
`GET /api/sessions/<sid>` (the API behind `dc sessions get <sid>`) must return
that session with the captured provider. The wait re-checks on daemon events
under `RL_OWNED_DEADLINE`; there is no sleep. The gate reads the provider's
session enumeration, which can lag the record restore trusts (Codex lists a
thread only after naming it), so it is deliberately at least as strict as
restore: a pass rules out a restart inside a provider persistence window.

A SETUP run prints one line per unowned tab, then the SETUP summary:

```text
== [cycle-1] every provider store owns its captured session before the restart
SETUP provider store did not own 01a09e84-… (tab 2b7402d3 rl-codex-worker, provider codex; sessions get: 404)
== restore lifecycle SETUP cycle=1 (no restart taken; not a restore verdict) run=target-check/evals/restore-lifecycle-…
```

A FAIL run after a proven setup looks like:

```text
PASS owned 5 agent sessions owned by their provider stores
== [cycle-1] full dev stop (reboot analogue) and start
FAIL identity tab 2b7402d3 (rl-codex-worker) agent_session_id: '01a09e84-…' -> None
```

## Forensics

`--lifecycle` inspects exactly one tab, so each cycle runs
`dc tab inspect --tab <id> --lifecycle --json` once per tab of the run and merges
the results into one JSON object keyed by tab id:
`lifecycle-cycle-N-before.json` (after the ownership gate, before the restart)
and `lifecycle-cycle-N-after.json` (once the restore report settled or failed).
A SETUP stop writes `lifecycle-cycle-N-setup.json`. A tab whose capture failed
is recorded as `{"capture_error": "<stderr>"}` with a `WARN` line; a capture
never changes the verdict.

On success, cleanup closes the Team, then closes the run's workspace with
`dc workspace close <id> --force`, because the standalone agent is not a Team
member and survives the Team close. `--force` is passed only after
`/api/workspaces` confirms the recorded id still carries this run's unique
`RESTORE-LIFECYCLE-<run>` name; any other workspace is never force-closed.

## Scenario

1. Workspace `RESTORE-LIFECYCLE-<run>` created with `--provision`, so the daemon
   creates its directory and every tab inherits it with directory trust; `team pair` gives a Claude Orchestrator
   and a Codex Planner; the Orchestrator spawns a Claude Worker and a Codex
   Worker; a standalone Claude agent and a plain shell join the workspace.
2. Every agent binds a session and completes one marker turn, so each provider
   store holds a resumable conversation.
3. Capture `/api/tabs/<id>/status` (`agent_type`, `agent_session_id`, `role`,
   `role_profile`, `team_id`, `parent_tab_id`, `title`) for every tab.
4. Per cycle: `POST /api/session/save`, `build.sh kill`, `build.sh dev-prebuilt --wait`.
5. Assert per cycle:
   - `state/recovery/last_restore_report.json` (written after the restart)
     lists every agent pane `restored: true, liveness: live`;
   - every captured field is unchanged behind the same tab id, no session is
     bound to two tabs, and the shell has no agent identity;
   - every agent answers a fresh marker turn, and identity is still unchanged
     afterwards (the resumed process is live, not a record).

The Team id is the Orchestrator's own `team_id` from its tab status, cross-checked
against `team list --json` (`{"teams": [...]}`). Every CLI JSON payload is saved as
`<label>.json` plus `<label>.stderr` in the run directory; a refusal or non-JSON
payload fails with both streams printed.

On success the Team, workspace and new plans are removed. Cleanup runs through
`dc` without `--as-tab`, so the caller is never a member of the Team or
workspace it closes (a coordinator cannot close the workspace it runs in). On
failure they are kept and the run directory
(`target-check/evals/restore-lifecycle-<run>`) holds the captures and markers.

## Pass condition

The script prints `== restore lifecycle PASS` and exits 0. Any `FAIL` line exits
non-zero and names the tab, field, and before/after values.
