# Lifecycle recovery oracle

This deterministic dev oracle proves two user-visible contracts through the dev-locked `dc` CLI wrapper:

- the Hubble-shaped final-Orchestrator provider exit observed on macOS and Windows becomes an `absent` `shell_ready` PTY, retains Team/role metadata, loses stale agent identity, is refused by `agent restart`, and is replaced through the original Team by one live owner;
- unlinking the final Orchestrator retains its Team and active Plan, and the documented `agent spawn --team ... --role Orchestrator` flow transfers the original Plan and owner history to one live replacement.

The runner subscribes to `/api/events/stream` before each action and evaluates state only after a matching state-change event. Its 45-second bounds are failure deadlines, never readiness signals. It contains no fixed-delay coordination, polling loops, or Playwright dependency.

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

<!-- revision: 5 -->
