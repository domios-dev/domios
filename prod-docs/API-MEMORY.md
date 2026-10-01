# Memory API

REST endpoints for memory items, retrieval, graph operations, workers, calibration, and synchronization.

#### Items, retrieval, and lifecycle

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| GET | `/api/mem/search` | `?q=...&kind=...&area=...&scope=...&limit=N&mode=hybrid_rrf\|vector_first_rerank&weight=0.30&rerank=true&decision_rerank=true\|false` | Hybrid retrieval. Default mode is `hybrid_rrf` (FTS5 + vector RRF fusion). `vector_first_rerank` uses vector recall plus keyword/temporal reranking; `weight` is `0.0..=1.0` (default `0.30`). `rerank=true` adds an LLM second pass. `decision_rerank` overrides `[memory.decision_rerank] enabled`: when applied, each hit carries `relevance` and the response adds `decision_rerank: {status: "applied", adapter, low_confidence, max_relevance}`; on failure the hits are the plain recall hits and the envelope is `{status: "fallback", reason}` (`no_adapter`, `unsupported`, `unavailable`, `deadline`, `adapter_error`). `rerank=true` with `decision_rerank=true` is a 400; `rerank=true` with the stage enabled only by config returns the legacy result with `{status: "skipped", reason: "legacy_rerank"}`. With `[memory.decision_rerank.sources] graph > 0` an applied response also tags each hit `via: "recall"\|"graph"` and adds `sources: {recall, graph}` to the envelope; fallback carries no graph or docs additions; docs > 0 also adds `via: "docs"` hits shaped as `{source: "doc", doc, section, anchor, pointer, relevance}` and includes `docs` in `sources`; applied responses carry no source metadata only when both sources are 0. With the stage off the response has no `decision_rerank` key. |
| POST | `/api/mem/decision/token` | `{adapter, token}` | Store a decision-adapter token under `<data dir>/decision-models/` (owner-only file). The response carries `adapter`, `present`, `path`, and `mode_ok`, never the token. Unregistered adapter: 404. Bearer HTTP only: the socket refuses `mem.decision.token.*` and mesh peers are denied. |
| GET | `/api/mem/decision/token` | `?adapter=<id>` | Token status: `adapter`, `present`, `path`, `mode_ok`. |
| DELETE | `/api/mem/decision/token` | `?adapter=<id>` | Remove the stored token; returns `{adapter, removed}`. |
| GET | `/api/mem/decision/status` | — | Decision re-rank config (no token), whether the configured adapter is registered, and each registered adapter with its primitives and token status. |
| POST | `/api/mem/items` | `{kind, title, body, scope?, summary?, area?, tags?, source?, importance?, replace?, skip_extract?, source_path?, pinned?}` | Save an item. `scope` is `global` or `workspace:<id>`; other values are refused. `pinned: true` sets the wake-up pin atomically. `skip_extract: true` suppresses automatic extraction. |
| GET | `/api/mem/items/{id}` | — | Get one item with its full body. Real reads bump `evidence_count` for skills, lessons, rules, facts, and preferences; worker reads do not. |
| PATCH | `/api/mem/items/{id}` | `{title?, summary?, area?, tags?, body?, importance?, pinned?, changed_by?}` | Partially update an item. Pin changes are audited. A nonblank `changed_by` overrides the actor header after sanitization. |
| DELETE | `/api/mem/items/{id}` | — | Delete an item. |
| GET | `/api/mem/items/{id}/history` | — | Read item history. |
| POST | `/api/mem/items/{id}/restore` | `{version?, changed_by?}` | Restore from history. When the latest kind change archived the item (calibration archive or merge), the prior kind comes back, the body of `version` too when given, and the item's merge relation is invalidated: `{restored, kind, body_restored, invalidated_relations}`. Otherwise the body of `version` (or the latest body change) is restored: `{restored, body_len}`. |
| POST | `/api/mem/merge` | `{keeper, lesser, body?, conflict?, changed_by?, worker_id?}` | Reversible merge in one transaction: optional new keeper `body`, the lesser archived with a `merge` history row, and a `merged_into` lesser → keeper relation (`supersedes` keeper → lesser with `conflict`), tagged `reason: "mem-merge"`. Never deletes. Pinned, `skill`, `rule`, `preference` and archived items are refused as the lesser (409). A dreaming worker (resolved from `X-CodeTerm-Worker-Id` or `worker_id`) is audited as `dreamer-rem:merge` and capped at `memory.dream_hygiene.merge_cap` merges per UTC day (429 past it). |
| GET | `/api/mem/pinned` | `?limit=N` | List wake-up-pinned items by importance, evidence count, and update time. Default limit is 100; maximum is 500. |
| GET | `/api/mem/taxonomy` | — | Read memory taxonomy. |
| GET | `/api/mem/tail` | — | Tail recent memory events. |
| DELETE | `/api/mem/forget/{area}` | — | Forget all memory in an area. |
| GET | `/api/mem/wake-up` | `?budget=N&l0_only=true&no_l0=true&workspace=<ws>&explain=true` | Return L0 + L1 critical context. Workspace-scoped items receive a `+0.5` L1 boost; `explain=true` appends per-item score details. The configured L1 budget defaults to 300 tokens. |
| GET | `/api/mem/stats` | — | Return kind counts, totals, and index health. |
| POST | `/api/mem/reembed` | — | Rebuild embeddings. |
| POST | `/api/mem/learn` | `{...}` | Learn from agent or user context. |
| POST | `/api/mem/promote/{id}` | — | Promote a learning to a memory item. |
| POST | `/api/mem/flush` | `{...}` | Flush a tab's pending conversation memory. |
| POST | `/api/mem/check-duplicate` | `{...}` | Check for duplicate memory. |
| POST | `/api/mem/dream` | `{...}` | Run memory consolidation. |
| POST | `/api/mem/extract-pending` | `{...}` | Extract pending memories. |
| GET | `/api/mem/dream/report` | — | Read the latest consolidation report. |
| POST | `/api/mem/sync` | `{...}` | Synchronize source-tracked memory. |
| GET | `/api/mem/sync/status` | — | Read memory synchronization status. |

**Actor and worker headers:**

- `X-CodeTerm-Worker-Id` is set automatically when the CLI inherits `CODETERM_WORKER_ID`. Worker reads skip the `evidence_count` bump so extraction and dream sweeps do not feed back into L1 scoring.
- `X-CodeTerm-Actor` is stored as `mem_item_history.changed_by` and defaults to `user`. The calibration sweep uses `dreamer-calibration:{promote|demote|archive|stale}`; a Dreamer merge uses `dreamer-rem:merge`.

**Extraction concurrency.** Automatic extraction is capped at four concurrent workers process-wide. Saves tagged `auto-extracted`, sourced from `extracted_from:*`, or carrying `skip_extract: true` do not schedule extraction.

#### Knowledge graph

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| POST | `/api/mem/graph/entities` | `{...}` | Create a graph entity. |
| POST | `/api/mem/graph/relate` | `{...}` | Create a graph relation. |
| GET | `/api/mem/graph/related` | `?id=...` | List related entities. |
| POST | `/api/mem/graph/invalidate` | `{...}` | Invalidate a relation while preserving history. |
| GET | `/api/mem/graph/timeline` | — | Read a graph timeline. |
| GET | `/api/mem/graph/traverse` | `?id=...` | Traverse a graph neighborhood. |
| GET | `/api/mem/graph/path` | `?from=...&to=...` | Find a graph path. |
| GET | `/api/mem/graph/tunnels` | — | List graph tunnel candidates. |
| POST | `/api/mem/graph/alias` | `{...}` | Add or merge a graph alias. |
| GET | `/api/mem/graph/full` | — | Return the full graph view. |
| GET | `/api/mem/graph/labels` | — | Return graph labels. |
| GET | `/api/mem/graph/edges` | — | Return graph edges. |
| GET | `/api/mem/graph/snapshot` | — | Return a compressed graph snapshot. |
| POST | `/api/mem/graph/layout/rebuild` | — | Rebuild graph layout. |
| GET | `/api/mem/pinned-positions` | — | List pinned graph positions. |
| PATCH | `/api/mem/pinned-positions/{mem_id}` | `{...}` | Set a pinned graph position. |
| DELETE | `/api/mem/pinned-positions/{mem_id}` | — | Delete a pinned graph position. |
| GET | `/api/mem/activity/recent` | — | Return recent memory activity. |

#### Graph self-repair

These whitelist-bounded Dreamer primitives audit every destructive operation to `graph_purge_log`. See `codeterm docs MEMORY` for the full Dreamer flow.

| Method | Path | Body | Description |
|--------|------|------|-------------|
| POST | `/api/mem/graph/repair/find-relations` | `{group_by:[…], limit:N}` | Find duplicate relation groups by a whitelisted subset of `from_id`, `relation_type`, `to_id`, `scope`, and `valid_from`. Read-only. |
| POST | `/api/mem/graph/repair/delete-relations` | `{rowids:[…], actor}` | Delete explicit relation row IDs and preserve their full payloads in the audit log. |
| POST | `/api/mem/graph/repair/merge-relations` | `{keep:N, merge:[…], actor}` | Fold relation rows into `keep`, taking maximum confidence and merging properties with `keep` winning collisions. |
| POST | `/api/mem/graph/repair/find-entities` | `{strategy:{kind:"exact_name"\|"alias_chain"\|"vector", threshold?, entity_type?}, limit:N}` | Find duplicate-entity candidates. Vector thresholds are `0.0..=1.0`. Read-only. |
| POST | `/api/mem/graph/repair/augment-entity` | `{id, patch:{…}, actor}` | Patch entity properties, re-embed, and audit the operation. |
| POST | `/api/mem/graph/repair/delete-entity` | `{id, force:bool, purge_history:bool, actor}` | Delete an entity. Active relations require `force`; `purge_history` also removes invalidated relations. Aliases and entity vectors are cleaned up. |
| POST | `/api/mem/graph/repair/orphan-vectors` | `{dry_run:bool, actor}` | Find or delete vectors with no matching entity or memory item. |
| POST | `/api/mem/graph/repair/orphan-blobs` | `{dry_run:bool, actor}` | Find or delete CAS blobs not referenced by an item. |

#### Calibration

| Method | Path | Body / Query | Description |
|--------|------|--------------|-------------|
| POST | `/api/mem/calibration/sweep` | `?force=true` | Run importance and archive calibration. Unless forced, it runs only on Sunday and rejects a repeat within six hours. It caps a session at 30 mutations and aborts candidate sets over 100. The `all` dream phase invokes it after the LLM phases. |
| GET | `/api/mem/calibration/log` | `?since=ISO8601&limit=N&count=true` | Read calibration audit entries, each with its `reason`. `since` must be RFC3339; the default limit is 50 and maximum is 500. `count=true` returns only the count. |
| POST | `/api/mem/calibration/stale` | `?dry_run=true&limit=N` | Staleness archive: items created over 365 days ago with no access, update, history row, relation created or invalidated, or touched active neighbour (item or entity) in that year. Pinned, `skill`, `rule`, `preference`, archived and importance >= 4 are excluded. `dry_run` lists `selected` ids with reasons and `matching` (the total) and writes nothing; `limit` sizes that list (default: tonight's remaining cap, max 1000). An apply needs `memory.dream_hygiene.stale_archive` (else `skipped_reason: "disabled"`), archives at most `stale_archive_cap` per UTC day as `dreamer-calibration:stale`, and returns `archived: [{id, reason}]`. The `all` dream phase runs it nightly after the weekly sweep and reports it under `stale`. |

#### Memory workers

| Method | Path | Body | Description |
|--------|------|------|-------------|
| GET | `/api/mem/workers` | — | List memory workers. |
| POST | `/api/mem/workers` | `{...}` | Spawn a memory worker. |
| GET | `/api/mem/workers/{id}` | — | Get worker status. |
| DELETE | `/api/mem/workers/{id}` | — | Kill a worker. |
| POST | `/api/mem/workers/{id}/progress` | `{...}` | Submit worker progress. |
| POST | `/api/mem/workers/{id}/report` | `{summary, diary?, finalization?, reason?}` | Submit a worker report. `summary` records that execution ended; `diary` + `finalization` (`succeeded`/`partial`/`failed`) + `reason` record artifact finalization, reconciled against the stored record. |
| GET | `/api/mem/workers/{id}/log` | — | Read the worker log. |

## Related documents

Memory skills, workers, sync, and dreaming: `codeterm docs MEMORY-SERVICES`. Service and activity endpoints: `codeterm docs API-SERVICES`.

<!-- revision: 11 -->
