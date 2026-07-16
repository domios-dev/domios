# Manager bootstrap bench

Run only against an exact-head dev daemon:

```bash
source ../_lib/lib.sh
bash run.sh
```

The runner uses `dc` only. It creates a fresh orchestration pair and staffed
manager fixtures, sends the two identity probes, checks role/project plus the
honest unbound or Team context, verifies the worker control probe is unchanged,
then removes every created pane and Team. System-prompt and fallback fixtures
are recorded separately when the configured providers expose both capabilities.

The offline shape suite records the distinction matrix: no-task managers use
the idle wake-up starter, task-bound workers retain their provider starter, and
equal inbound messages with distinct ids remain distinct (rendering identity is
owned by task_ossj98).

Do not run while dev 7686 is stopped or its health build differs from `HEAD`.

## JSON contracts and fixtures

`dc agent providers --json` is a top-level provider array. `dc team list --json`
is an object with a `teams` array. `dc pane chat --json` is an object with
`messages`; assistant content can be a string, a content-block object, or a list
of both. The captured envelopes used by the executable offline parser harness
live in `fixtures/`; refresh them only from a live exact-head dev daemon.

The first-response wait subscribes to the supported per-pane chat SSE event,
with a 45-second failure deadline. It does not use timer polling. Every CLI JSON
response is captured before parsing, so a parser failure cannot close a producer
pipe early.

<!-- revision: 1 -->
