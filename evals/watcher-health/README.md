# watcher-health

End-to-end smoke bench for chatBackend watcher mode. It targets the dev daemon
on port 7686 and proves spec acceptance criteria 1-3:

- a watcher-capable `lmstudio` shell can be spawned as `--role Watcher` under an
  orchestrator with a machine charter;
- the watcher transcript receives a `context_request` and a `watcher_verdict`;
- a deliberately quiet orchestration receives exactly one watcher-origin nudge
  across two ticks, proving both nudge execution and the cooldown path.

## Prerequisites

- Dev CodeTerm is already running on `127.0.0.1:7686`.
- LM Studio has a model loaded and its local server is reachable at
  `http://localhost:1234/api/v1/models`.
- The `lmstudio` plugin installed in dev advertises
  `chatBackend: { spawnable: true, watcher: true }`. During local development,
  install the plugin from the sibling checkout:

```bash
cd /Users/kandrey/Developer/codeterm-canvas
source evals/_lib/lib.sh
dc plugin install lmstudio --from-dir ../codeterm-plugins/lmstudio
```

## Run

```bash
cd /Users/kandrey/Developer/codeterm-canvas
bash evals/watcher-health/run.sh
```

The script sources `evals/_lib/lib.sh`; every CodeTerm CLI call goes through
`dc` and is pinned to dev. It never invokes the bare `codeterm` CLI, so a
prod-hosted agent tab cannot leak calls to port 7685.

Missing LM Studio or a non-watcher plugin is reported as `SKIP ...` and exits
zero. Product assertions print `PASS`/`FAIL` summary lines and a failing
assertion exits non-zero.

## Oracle And Bounds

The bench uses a custom inline charter with a 45 second stall threshold so the
silent worker fixture genuinely qualifies as stalled inside the run. It reads
`dc --json tab chat` and checks `messages[].type` for `context_request` and
`watcher_verdict`; nudge counting uses structured `fromLabel`/`from_label`
metadata on the orchestrator transcript.

Default timing is a 95 second sleep, then two bounded 45 second transcript
polls. That keeps the normal worst case under five minutes including setup and
cleanup. Set `WH_WAIT_SECS` or `WH_POLL_SECS` only for diagnosis; do not raise
them in CI or orchestrated QA.
