---
name: agent-restart
description: "Exercise graceful, forced, and recovery paths for a CodeTerm agent restart."
---

# Agent-restart testing playbook

How to test the `codeterm agent restart` flow against a dev instance —
both the graceful path (default) and the forced path (`--force`).

Cross-references:
- Service code: `src-tauri/src/service/agent_restart.rs`
- REST route: `src-tauri/src/api/tab_agent_routes.rs`; handler: `src-tauri/src/api/agent_routes.rs::agent_restart_handler`
- CLI: `cli/src/main.rs::Agent { Restart { ... } }`
- API doc: [`prod-docs/API.md` §Agent Restart](../../prod-docs/API.md#agent-restart-post-apitabsidagentrestart)

## What the flow does

Agent restart takes an observed live provider through its graceful exit command and then auto-resumes
the same Tab; a provider that already exited to its shell is recovered in place (section 6). Update shutdown is deliberately different: it tears down owned
PTY process trees without sending `/exit`, preserves each Tab's session binding
and lifecycle lineage, persists the snapshot, and resumes after relaunch. Steps:

1. Lookup the Tab and provider (Claude / Codex / ...).
2. Send the provider's exit command (Claude: `/exit`, etc.) only when the Tab
   reads `idle`. A busy composer would queue it as a user turn, so a working
   agent skips it and is stopped at the process by step 4.
3. Wait up to `grace_secs` (default 10) for the PTY to exit cleanly.
4. If `--force` is set, the Tab was busy, or the timeout fires, kill the PTY.
5. The existing Tab lifecycle detects the exit and respawns the agent
   with `--resume <session_id>` if a session id was recorded — otherwise
   a fresh session is started. The command is built from the Tab's recorded
   spawn (`restorable_launch`: model, effort, arguments, identity
   instructions) through the same seam restore uses
   (`restored_fresh_launch_input`), so restart and restore cannot diverge.
   Coverage: `restart_preserves_launch_args`.

## Setup

Run a dev instance with at least one Claude Tab so there's an agent to
target:

```bash
./build.sh dev
# Open a workspace, spawn `claude` in a tab, let it idle
TAB=$(curl -s http://127.0.0.1:7686/api/tabs \
  -H "Authorization: Bearer $TOKEN" \
  | python3 -c "import sys,json;print([p['id'] for p in json.load(sys.stdin) if p.get('agent_type')=='claude'][0])")
echo "Target tab: $TAB"
```

## 1. Graceful restart

```bash
~/.codeterm-dev/bin/codeterm agent restart --tab $TAB
```

Expected:
- The Tab prints whatever `/exit` produces (Claude shows a goodbye
  banner), then the PTY closes.
- Within ~1s the same Tab respawns with a new PID; if the previous
  session had a `claude_session_id` recorded, the new process is
  launched with `--resume <id>`. Verify with:
  ```bash
  curl -s http://127.0.0.1:7686/api/tabs -H "Authorization: Bearer $TOKEN" \
    | python3 -c "import sys,json; p=[x for x in json.load(sys.stdin) if x['id']==\"$TAB\"][0]; print(p)"
  ```
  `pty_id` should change; `claude_session_id` should remain stable.

## 2. Forced restart on a hung agent

To simulate a hang, run a Claude Tab and `Ctrl-Z` it (Unix) or send a
SIGSTOP to the PTY's child process (Windows: use Process Explorer to
suspend the process). Then:

```bash
~/.codeterm-dev/bin/codeterm agent restart --tab $TAB --force --grace-secs 3
```

Expected:
- The graceful exit times out (the suspended process won't respond),
  the PTY is killed at the 3s mark.
- The Tab respawns. `session_was_stale` in the response indicates
  whether the recorded session id was valid; if the JSONL was deleted
  between runs, the field is `true` and the new Tab starts a fresh
  session.

## 3. Rate-limit guard

The service throttles restarts per-tab to avoid restart loops. Hit
the endpoint twice in quick succession:

```bash
for i in 1 2; do
  curl -i -X POST "http://127.0.0.1:7686/api/tabs/$tab/agent/restart" \
    -H "Authorization: Bearer $TOKEN"
done
```

The second call returns `429 Too Many Requests` with a `Retry-After`
header. Wait for the cooldown, retry, and the call should succeed.

## 4. Error cases

- **`404`** — Tab id doesn't exist (typo, closed Tab).
- **`400`** — the Tab has no agent and no verified prior session, its
  exited session is not owned by exactly one provider, it has no live
  shell, or the provider doesn't support restart.
- **`500`** — internal failure (PTY spawn error). Check
  `~/.codeterm-dev/logs/codeterm.log` for the trace.

## 5. Use from another agent

A common pattern: an Orchestrator notices a Worker is stuck and asks
the system to restart it.

```bash
codeterm agent restart --tab $WORKER_TAB_ID --grace-secs 30
```

For a Worker on another peer, add `--mesh PEER` and pass its peer-side tab id
explicitly. A mesh restart refuses without `--tab` and never reads the caller's
local `CODETERM_TAB_ID`.

```bash
codeterm agent restart --tab "$WORKER_TAB_ID" --grace-secs 30 --mesh PEER
```

The Orchestrator's Tab keeps running; only the Worker's PTY cycles.
Reports the Worker had already sent are preserved (the in-memory
`ReportStore` is not tab-scoped to PTY id; it's keyed on the parent +
child Tab ids which don't change across restart).

## 6. In-tab provider relaunch and recovery

A provider started by hand inside a tab's surviving shell (for example through
`codeterm tab exec`) has no launch pipeline to seal it. When its hook session
start echoes the tab's current binding nonce with an accepted session, while no
launch owns the tab (`agent_booting` false), the tab still has a pre-agent stage
(none, `pty_*`, `reader_ready`, `shell_ready`, `pty_reattached`), and its PTY is
live, `reconcile_boot_stage_with_session_binding` records `agent_ready`. From
that transition the tab is observed `live`, public and Chat UI sends pass the
ready gate, a user send parked while it was not ready is delivered once,
`agent restart` is accepted, and a later provider exit is observed and clears
the identity. Launch-owned, provisional, failed, and already-ready stages keep
their own rules. Without this rule the epoch recorded `session_bound` but never
`launch_ready`, restart refused the tab as not observed, and Chat UI sends were
parked as `tab_busy_refused`.

A provider exit observed outside a shutdown clears the provider, session and
live nonce, retiring the nonce into `prior_session_binding_nonces` tagged with
its session, and leaves an `absent` `shell_ready` PTY with its role and Team.
`agent restart` recovers that tab in place. It takes the newest lineage session
(the journal's newest `session_bound` when the tab has no lineage), requires a
journaled nonce-verified binding for it and exactly one provider owning its
session record (`provider_for_claimed_session`), skips the graceful exit and
teardown, retires the shell's boot stage, and relaunches that provider with
resume through `launch_agent_in_background` (origin `exit_recovery`) on the same
PTY, so the nonce rotates and boot stages and the journal behave as in a launch.
The response carries `recovered_in_place: true` with equal `old_pty_id` and
`new_pty_id`. The verified binding survives compaction: a checkpoint that
carries a binding never shares its `seq` with a retained `session_bound`, and a
rolled-back checkpoint stays as that session's proof. A refused journal tail is
quarantined into `tab-lifecycle.refused.jsonl` on open or compaction, so later
bindings replay. Coverage: `exit_clear_keeps_verified_binding_in_lineage`,
`exit_recovery_resumes_roleless_and_team_tabs`, and the codeterm-pty
`compaction_replay` tests. Refusals happen before acceptance with `exit_recovery_no_live_shell`,
`exit_recovery_no_verified_session` or `exit_recovery_session_not_owned`; then
spawn a fresh agent, or replace a missing Team Orchestrator through
`codeterm agent spawn <provider> --team <team-id> --role Orchestrator`. The
General Agent keeps its own restart control. `tab restart` refuses an absent tab
that still carries an agent role (`shell_restart_agent_member`): a hand-typed
provider resume there bypasses the launch builder and starts the agent without
its granted permission bypass, sandboxed away from the daemon.

A provider resumed by hand in that surviving shell (the `claude --resume <sid>`
line the provider prints on exit) inherits the exited launch's
`CODETERM_SESSION_BINDING_NONCE`. When its session-start hook reports that
nonce, the tab has no live nonce, no launch owns it and its PTY is live, the
claim reinstates the nonce as the tab's own, the binding is verified, a
lifecycle operation opens so `session_bound` is journaled, and the in-tab rule
above records `agent_ready`, so the tab becomes sendable. It still runs without
the recorded launch arguments, so prefer `agent restart`. Coverage:
`manual_resume_in_shell_is_adopted`.

The same recovery holds across a daemon restart. The session snapshot persists
the binding lineage (`agent_identity.prior_session_bindings`) and restore keeps
it for every tab, so a team member that exited to its shell before shutdown
restores as a shell with its role, Team and lineage intact. Restore does not
relaunch it from the snapshot: no agent was running, and the proof that its
session was verified lives only in the lifecycle journal, which restore never
uses as authority. Once that shell is ready, exit recovery resumes it once. The
restore report names the pane `agent_exited_before_shutdown` with
`exited_session_id` and logs the recovery command, and the journal records one
`restore_awaiting_agent_restart` per member and session, not one per boot.
`codeterm agent restart --tab <id>` recovers that exact lineage session in place
as above. A relaunch (restore, resume, restart, exit recovery) of a task-bound
Worker owes no dispatch startup handshake: its persisted task binding alone
never fails the launch for a missing startup waiter, and every launch failure
logs `[launch] Tab <id> launch failed ... at <stage>`. Coverage:
`restore_resumes_idle_worker`,
`restore_contract_exited_member_*`,
`restore_contract_startup_flags_an_exited_member_and_journals_its_decision_once`,
`restore_contract_restored_lineage_recovers_the_session_that_exited_before_shutdown`,
and the exited-member phase of `evals/restore-lifecycle`.

Routed tab messages carry a return-address marker only when the target record
declares an agent (`agent_type` set or a launch in flight); otherwise the body
is shell input. A live provider behind a record that no longer declares it
receives the raw text in its composer, where a leading `/` is a provider command.

Manual check: exit an agent to its shell and confirm the caller's workspace map
from `codeterm tab list` shows the tab `absent`, then run `codeterm agent restart --tab $TAB`. It returns
`agent_ready` with `recovered_in_place: true` and the prior `session_id`, the
dev log shows `[agent-launch] origin=exit_recovery`, the tab is `live` on the same
PTY, and a Chat UI send is delivered without `tab_busy_refused`.

## Internal references

Moved out of the embedded `codeterm docs` pages because they describe CodeTerm's own source:

- [references/lifecycle.md](references/lifecycle.md) — restore, liveness and lifecycle-journal internals
- [references/shell-integration.md](references/shell-integration.md) — per-shell cwd integration
