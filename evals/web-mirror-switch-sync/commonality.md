# What every failing run has in common

This file is the instrument, and it is deliberately written **before** any
feature is named as the cause. Nothing here is a conclusion. `run.sh` appends
one row per run to `target-check/evals/web-mirror-switch-sync/runs.tsv`; the
columns below are that row. A cause is named only after the rows exist and one
column discriminates pass from fail.

The failure this plan already has an entry for: naming the first feature that
came to mind and being wrong an hour later. The guard against it is that the
column list is fixed now and filled later.

## Columns recorded per run

| Column | Meaning | Why it discriminates |
|---|---|---|
| `origin` | `local` or `peer` | Andrey reports "especially with other machines". If failures are peer-only, the local and mesh causes are distinct and the mesh half is its own boundary. |
| `tabs` | tabs open at once | Separates a fan-out cost that scales with tab count from one that does not. |
| `switches` | switches issued in the window | With `elapsed_ms`, gives the observed switch rate. Not paced -- measured. |
| `switch_rate_hz` | `switches / elapsed_ms * 1000` | The rate is an *observation*, never a control input. No sleep sets it. |
| `overflow_fired` | count of `overflow` frames seen | The only loss signal on the wire. A failure with zero overflows is a different mechanism from a failure with one. |
| `overflow_reason` | `boot-changed` / `ring-evicted` / `lagged` | Three different server paths. They must not be collapsed. |
| `elapsed_ms` | wall time of the storm window | Distinguishes a rate effect from a duration effect. |
| `resync_fanout` | `fireResync()` listener invocations | Measures the unscoped broadcast directly. |
| `mux_refetches` | `GET /api/mux/state` calls issued | The refetch amplification, counted rather than inferred. |
| `client_gen` / `daemon_gen` | store generation vs daemon generation | A stale-snapshot drop shows up here and nowhere else. |
| `diverged` | assertion result | The oracle: client tab set/focus vs `GET /api/mux/state`. |
| `divergence_kind` | `missing` / `extra` / `focus` / `title` | Which projection drifted; a missing tab and a wrong focus are not one bug. |

## How to read the table

1. Sort by `diverged`. Take the failing rows only.
2. For each column, ask: does every failing row share this value, and does at
   least one passing row differ? Only a column that answers yes to both is a
   discriminator. A column constant across *all* rows is a property of the
   harness, not of the bug.
3. If `origin` is the only discriminator, the local and mesh causes are
   distinct. Report that as a boundary and stop; do not fix two things in one
   lane.
4. Only after step 2 yields a discriminator may a source-level mechanism be
   named as the cause, and it must explain that column.

## Status

Rows exist for `origin=local` only, at two concurrencies. `origin=peer` has no
rows at all, so the column Andrey's report points at cannot be compared. No
cause is named. See `FINDINGS.md`.
