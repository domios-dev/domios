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

## Scenario

1. Workspace `RESTORE-LIFECYCLE-<run>`; `team pair` gives a Claude Orchestrator
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
