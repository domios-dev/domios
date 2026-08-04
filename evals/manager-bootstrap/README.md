# Manager bootstrap bench

Fast live smoke (Codex Orchestrator + Claude Planner):

```bash
npm run test:manager-bootstrap
```

Full provider matrix (Claude, Codex, OpenCode, and Cursor as singles and across
two orchestration pairs):

```bash
npm run test:provider-matrix
```

Narrow a diagnosis to one provider or pair without editing the runner:

```powershell
$env:MATRIX_PROVIDERS='opencode'; $env:MATRIX_SKIP_TEAMS='1'; npm run test:provider-matrix
$env:MATRIX_PROVIDERS=''; $env:MATRIX_PAIRS='opencode+cursor'; npm run test:provider-matrix
```

The single-agent oracle accepts only an assistant message containing the
computed result; that result is deliberately absent from the task text. The
team oracle requires exactly one starter user turn and an assistant
acknowledgement of both roles and the counterpart tab. Both runners subscribe
to tab/chat events before spawning and use deadlines only as failure bounds.

This single process checks dev health and exact-head identity, subscribes to the
event stream before spawning, creates an isolated workspace and pair, proves
each manager received exactly one starter turn and acknowledged the correct
role/counterpart, then removes the workspace. It does not poll or sleep.

Run only against an exact-head dev daemon:

```bash
source ../_lib/lib.sh
bash run.sh
```

The runner uses `dc` only. It creates a fresh orchestration pair and staffed
manager fixtures, sends the two identity probes, checks role/project plus the
honest unbound or Team context, verifies the worker control probe is unchanged,
then removes every created tab and Team. System-prompt and fallback fixtures
are recorded separately when the configured providers expose both capabilities.

The offline shape suite records the distinction matrix: no-task managers use
the idle wake-up starter, task-bound workers retain their provider starter, and
equal inbound messages with distinct ids remain distinct (rendering identity is
owned by task_ossj98).

Do not run while dev 7686 is stopped or its health build differs from `HEAD`.

## JSON contracts and fixtures

`dc agent providers --json` is a top-level provider array. `dc team list --json`
is an object with a `teams` array. `dc tab chat --json` is an object with
`messages`; assistant content can be a string, a content-block object, or a list
of both. The captured envelopes used by the executable offline parser harness
live in `fixtures/`; refresh them only from a live exact-head dev daemon.

The first-response wait subscribes to the supported per-tab chat SSE event,
with a 45-second failure deadline. It does not use timer polling. Every CLI JSON
response is captured before parsing, so a parser failure cannot close a producer
pipe early.

<!-- revision: 3 -->
