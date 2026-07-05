---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_5335168810504d0e9cfc1fdb444dcb2a
handle: mergeable-storage-research-ms_5335168810504d0e9cfc1fdb444dcb2a
title: decision
---

MIGRATION = idempotent deterministic 'explode' on load (NO Date::now/Math.random — same rule as §17.3). Monolith plan.json -> task per tasks/<id>.md, stride per strides/<task>.jsonl, link/canvas one-per-file; plan.json becomes slim meta. Mirrors §17 hydrate_and_migrate migrated-flag; tasks/ dir + slim plan.json = already migrated -> re-run no-op.
