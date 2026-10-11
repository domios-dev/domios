# OpenCode spawn and lazy-session restore

The `run-codex.sh` entry point also runs this serialized track in both GUI PATH
and normal modes, after the Codex phases. It uses the same restartqa instance,
approved SHA, scratch workspace and lifecycle operation. Wait for the explicit
Orchestrator handoff “restart bench is yours” with the final SHA before running.

| Phase | PASS condition | Evidence in each mode directory |
|---|---|---|
| 8 spawn | Two task-bound Workers (default and cheap) and one plain roleless spawn (default); distinct authenticated sessions, agent_ready, startup ACK/tool result, Worker task ACK, startup reply visible in Chat, no Starting | `opencode-spawn*.json`, `opencode-models.json`, `opencode-providers.json`, `opencode-timing-*.json`, `opencode-launch-observations-*.json`, `opencode-browser-startup-*.stdout` |
| 9 first turn | Each marked task appears exactly once in its own session and zero times in other sessions; Browser already viewing the tab shows reply without reload | `opencode-first-*.json`, `opencode-live-*.json`, `opencode-first-after-*.json`, `opencode-browser-first-*.stdout` |
| 10 idle restart | All tabs Idle before stop; same lazy sessions, retained Chat rows and user turns; zero new continues and zero delivered continue logs | `opencode-idle-*.json`, `opencode-idle-dev.log`, `opencode-browser-opencode-idle-*.stdout` |
| 11 Working restart | All three OpenCode tabs Working before stop; same lazy sessions, retained rows, exactly one new own-session continue each, one delivered log each, numeric original task continues afterward; other idle agents receive zero continues | `opencode-working-*.json`, `opencode-working-dev.log`, `opencode-browser-opencode-working-*.stdout` |

Models come from `dc agent models opencode` and the provider's advertised
`default_model`. Default launches omit `--model`; the cheap launch explicitly
selects a distinct advertised free/cheap/affordable entry. `RL_OPENCODE_MODEL`
can override that cheap choice. All probes are read-only and non-user-facing.

Timing uses daemon `first-composer` telemetry measured from launch request;
missing timing fails the measurement prerequisite. Binding and agent_ready
latencies are observed upper bounds from event-driven inspections, named as
such. Event and status observations are retained, including waits or prompts.
Readiness above 15 seconds is an advisory finding, never a readiness heuristic.

`opencode-findings.json` records slow readiness, model discrepancies, prompts,
missing/duplicate transcript rows, Starting and every hard failure with evidence.
`opencode-observation-limits.json` states visual limits: discrete accessibility
snapshots cannot exclude transient flicker. Review these snapshots during the
live run and add any observed flicker or prompt to the findings report; never
claim an unobserved visual behavior as a pass. Model ids and detected display names are recorded separately. Unequal or
unknown values are an unverified-model finding for live review, because a short
display name alone cannot prove that the wrong model ran. HTTP success alone is not a Chat PASS.

Normalized Chat rows come from `dc tab chat`, whose API reads only the tab's
bound provider session. Each read is bracketed by inspections that reject a
session change. User rows count delivery; assistant echoes and tool rows do not.
Continuations are checked both in those rows and in per-tab DEV log outcomes.
