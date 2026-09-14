# Lifecycle recovery oracle

This deterministic dev oracle proves three user-visible contracts through the dev-locked `dc` CLI wrapper:

- the Hubble-shaped final-Orchestrator provider exit observed on macOS and Windows becomes an `absent` `shell_ready` PTY, retains Team/role metadata, and loses stale agent identity. Its nonce-verified session has no provider session record, so `agent restart` refuses it before acceptance with `exit_recovery_session_not_owned` naming that session, the tab stays absent with its Team, and it is replaced through the original Team by one live owner;
- a roleless stub provider started in a dedicated cwd writes its own provider session record, binds that session, and exits to its shell. `agent restart` recovers it in place: the response reports `agent_ready`, `recovered_in_place`, `session_action: resume`, the same session id and equal old/new PTY ids. The tab is then observed `live` at `agent_ready` on that session, with `launch_ready` journaled after the restart in the operation that bound it; a `send` reaches the recovered stub's screen, and the dev log records no `tab_busy_refused` for the tab. Cleanup removes the stub's session record with its cwd;
- unlinking the final Orchestrator retains its Team and active Plan, and the documented `agent spawn --team ... --role Orchestrator` flow transfers the original Plan and owner history to one live replacement.

The exit fixture establishes an owned Team and active Plan through the authenticated API before the stub provider exits, matching an established Orchestrator rather than a forming-Team startup failure. The runner subscribes to `/api/events/stream` before each action and evaluates observed liveness through the ordinary all-tabs projection only after a matching state-change event; the agent-only projection intentionally omits an absent tab after its provider identity is cleared. CLI assertions use `dc tab list --all-workspaces`, the compact command that carries computed liveness, rather than the raw `tab inspect` record. Its 45-second bounds are failure deadlines, never readiness signals. JSON assertions use the existing Node.js runtime, so the drill does not require `jq`. Cleanup uses an explicit General-Agent caller outside the fixture workspace. It contains no fixed-delay coordination, polling loops, or Playwright dependency.

Canonical focused coverage must precede the drill: default-program PowerShell and pwsh builders remain argument-free; a configured shell nonce rejects naked D, wrong-nonce E+D, and replayed D while accepting one matching E+D; and a forced Plan-takeover failure removes the replacement anchor without changing the retained Team or Plan owner/history.

Start the exact-SHA dev build with the debug stub enabled for `claude`, then run the oracle from Git Bash:

```powershell
$env:CODETERM_STUB_AGENT_CMD = "node $((Get-Location).Path)/evals/lifecycle-recovery/stub-agent.mjs"
$env:CODETERM_STUB_AGENT_PROVIDERS = "claude"
.\build.ps1 dev
```

```bash
bash evals/lifecycle-recovery/run.sh
```

The runner refuses a dev daemon whose `/api/health` `build_sha` differs from `git rev-parse HEAD` and emits one distinct `PASS` row per incident.

<!-- revision: 12 -->
