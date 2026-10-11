# Restore Lifecycle

Proves that a Team of agent tabs survives full dev restarts (the reboot
analogue) behind the same tab ids, with the same provider, session, role, team
and title. DEV only (the selected named instance or default port 7686); every CLI call goes through the `evals/_lib`
`dc` wrapper.

## Codex release regression matrix (#557, #555)

The dedicated [Codex matrix](CODEX.md) runs all seven phases in GUI PATH and
normal modes, scoped to mobileqa/7690, after the Orchestrator supplies the
merged release SHA. The older mixed-provider scenario below remains separate.

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
- `RL_EXITED_MEMBER` (`1`) — run the exited-member phase after the cycles; `0`
  skips it.

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
6. Exited-member phase: the Claude Worker receives `/exit` and the bench waits
   until its status keeps role but has no provider or session. One more restart
   cycle, then:
   - the other agent panes restore live;
   - the Worker's pane reads `restored: false`,
     `drop_reason: agent_exited_before_shutdown`, and `exited_session_id` equal to
     its captured session, while its status still has no provider or session
     and keeps its role and Team (restore relaunched nothing);
   - `dc agent restart --tab <worker> --json` returns `recovered_in_place: true`,
     `agent_ready: true`, that `session_id`, and the same old and new PTY;
   - every tab's identity matches `identity-before.json` again, and the Worker
     answers a marker turn in the resumed session.

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

### Controlled roleless restore authentication fixture

For a shared owner DEV, use a controlled restart scheduled by the Orchestrator;
do not invoke `run.sh`, which owns its own restart cycles. Source
`evals/_lib/lib.sh` and use `dc` for every probe command.

Create a provisioned scratch workspace and a roleless agent declaring
`session_starts_on_first_user_turn`. Have it run `codeterm identify` and
`codeterm workspace current`, then wait. Capture that exact tab with
`dc tab inspect --tab <id> --lifecycle`: require an authenticated binding
(`reported_session_binding_nonce == session_binding_nonce`, both nonempty).
After the Orchestrator restarts DEV, inspect the same tab and session without
sending a turn that could hide a missing restore bootstrap. Require a new,
authenticated nonce and exactly one new restore binding turn in its transcript.
Then ask the agent to execute the same commands and require its actual successful
results. A restored delegated member additionally needs its Team command to
authorize on its existing assignment. A missing binding must instead leave
`launch_binding_unconfirmed` and a standalone recovery incident; an idle
composer alone is not a pass. Preserve unresolved delivery records and verify
that their payloads never appear again after restore.

## Close/Undo release regression fixture

Run these cases on the rebuilt DEV release SHA after the restart bench, using
`source evals/_lib/lib.sh` and `dc` for every probe and mutation. Record the CLI
compatibility verdict, lifecycle inspection, exact session id, visible history,
and model/effort before close and after Undo/Recently Closed restore.

| Case | Trigger | Observable pass condition |
|------|---------|---------------------------|
| Fresh OpenCode | Route a pad-backed send before any UI composer turn | Send is accepted or held; exactly one own-session user turn and contextual answer appear. |
| OpenCode 1.18.35 | Close, then Undo | Compatibility is supported; the original session id, prior messages and contextual answer remain. |
| Immediate Undo send | Make a pad-backed contextual send the next operation after Undo, without waiting for readiness | Send is held against the incoming launch; after its own session authenticates, exactly one user turn and contextual answer appear. |
| Immediate relaunch send | Send when a confirmed restart/resume picker closes, before readiness | Send is held; the new launch authenticates the saved session and receives exactly one user turn and contextual answer. |
| Failed pending launch | Hold a send during a launch that then fails before input | One `launch_ended_before_write` settlement reports `bytes_written: false`; no payload reaches this or a subsequent launch. |
| Aider | Close, then Recently Closed restore; route input at reader_ready | Input is held until exact saved history-marker binding, then written once; original history and contextual answer appear. |
| Grok | Close, then Recently Closed restore; route input before binding | Input is held until current-PTY PID-registry binding, then written once; original session history and contextual answer appear. |
| Codex | Confirm low effort, close, then Undo and complete one reply | Original session and model remain; requested/resolved effort remain low; actual effort returns to low with actual_source session-owned. |
| Unresumable conversation | Restore a saved conversation unavailable to its provider | UI shows a restore failure with a reason; no replacement session starts. |
| Missing conversation | Provider confirms the saved session is absent and journal or exact provider content proves a user turn | Card retains Start new session and Keep shell; no automatic replacement. |
| Empty missing session (Volta) | Rebuild DEV and restore Restoreprobe tab `32ffb66f` with no ConversationStarted or exact provider user content | Same tab becomes a fresh Claude Manager automatically; recorded provider, model, effort, permission/runtime mode, role/profile, Team/plan, cwd, title and launch prompt remain; one quiet system notice appears in Chat; no failure card. |
| Unknown restore evidence | Provider check or transcript read is inconclusive, journal is unreadable/corrupt, or launch parameters were not captured | Failure card remains and the saved session binding is preserved. |
| Explicit fresh recovery | Provider confirms the saved session is absent; click Start new session | One fresh session becomes ready in the same tab; tab id, title, role, Team, plan binding, cwd, model and effort remain; dead session and exact restore flag are cleared; lifecycle names the old session. |
| Existing unresumable session | Provider still owns the exact session but cannot resume it | Card offers Keep shell without Start new session; explicit `dc agent restart --tab <id> --fresh` refuses without changing the binding. |

Use a unique opaque token in the first turn and require the post-restore answer
to recall it. An accepted send or reader_ready alone is not a pass. Send one
additional routed turn after readiness; check both turns appear exactly once.
Unknown OpenCode versions remain blocked for direct SQLite reads; do not mark
such a run supported merely because the database columns look familiar.

For Codex, capture `dc tab inspect --tab <id> --lifecycle --json` before close,
immediately after Undo, and after the contextual reply completes. Runtime
effort may be unverified immediately after restore; the completed reply must
confirm it without a model/effort change or an extra verification turn.
