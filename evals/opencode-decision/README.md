# OpenCode decision proof

Owner-run canonical live leg, after focused gates and the real merge. Start the named `ocdec` DEV through the dev-run skill; use its printed API port below. Never select this adapter in PROD or default DEV.

```bash
export CODETERM_INSTANCE=ocdec
export CODETERM_API_PORT=THE_NAMED_DEV_PORT
source evals/_lib/lib.sh
dc supporter models
dc supporter use opencode --model PROVIDER/MODEL
dev_run node evals/opencode-decision/latency.mjs 'seeded named DEV fixture query' 20
dc supporter use typesafe-jev
```

Seed a secret-free memory fixture that the query finds before measuring. The probe verifies applied adapter results and at least twenty physical calls, then reports end-to-end request p50/p95 against five seconds. A fallback, skipped/cached decision or missed target fails rather than becoming a latency success. Record the raw exit and JSON result. Restore `typesafe-jev` even if the probe fails.

Also create and fire a one-step ask Blueprint through the current canonical Blueprint CLI. Require its journal to show `decided_by=adapter`; an accepted request is insufficient. Record a Supporter activity entry and the named DEV health/build SHA. Verify PROD and default DEV still select `typesafe-jev` without changing them.

For 1.12.4 the owner selected `opencode run --format json --pure`: async process completion already has a bounded host event wait. A reusable serve backend remains a later option and needs its own readiness/lifetime boundary; the archived Router's sleep/heartbeat/log polling is not reused. No serve performance comparison is claimed.

Mapping and confinement derive from `rollacode/codeterm-plugins` branches `feat/domios-router` at `a39224ce5a402c0545329b8737c018c7517e77e6` and `feat/router-coding` at `ae1420df878a692444325526818e69703cf1499f`, adapted to the existing typed DecisionModel contract and a tool-less process.
