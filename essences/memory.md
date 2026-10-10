# Memory: durable knowledge and recall

Memory keeps facts, notes, preferences and skills available to agents across conversations. Hybrid search, the knowledge graph and nightly consolidation serve that continuity. The [core](core.md#core:memory) owns the service responsibility; the [capability map](capabilities.md) records delivery status.

Persistent memory lives in Domios, never in provider side stores. It is English and its bodies carry no dates. Skills are registered through memory, never written directly into provider skill directories. Conversations are searched as sessions; durable facts are searched as memory.

## Decision recall

A connected Supporter adds a second stage that judges how directly and usefully each candidate answers the query. The first stage retains ordinary hybrid retrieval; the [Supporter Agent](supporter-agent.md) judges candidates independently and orders the pool by absolute relevance. Each candidate receives one yes/no probability judgment when supported, otherwise a normalized score. Ties keep the original order.

- Ordinary search callers keep their existing query, filters and limit. A connected Supporter enables the stage; a caller may override it diagnostically for one search. Without a connected adapter, the response is byte-identical to ordinary recall.
- Relevance changes order, never eligibility: every pool candidate remains eligible for the returned list, cut only to the caller's limit. Every hit returned by an applied stage carries `relevance`. `low_confidence` flags that the maximum relevance is below the configured threshold; it never removes results.
- The second stage is all-or-nothing under one bounded failure deadline, defaulting to 3 seconds. On unavailable or failed decision scoring, the caller receives exactly ordinary recall hits for the original query and limit. An unregistered adapter id, unsupported decision capabilities, an adapter error, or expiry adds a fallback reason in the decision envelope. Missing credentials also fall back when the pool requires scoring. A partially judged pool is never returned.
- Gathering and judging candidates record no access. Only memory hits actually returned record access, once per item, using ordinary memory access accounting. Additional candidates never inflate usage or evidence merely by entering the pool.
- Memory > Settings controls recall; the memory search CLI and API expose whether the stage applied, fell back or was skipped. Shared credentials and adapter status belong to the Supporter Agent. Explicitly requesting both decision recall and legacy reranking is refused; legacy reranking requested while decision recall is selected by the connected adapter runs with a skipped reason.

## Optional candidate sources

The ordinary recall pool defaults to 30 candidates and expands when needed for the caller's limit, within the retrieval cap. Optional sources can supply additional candidates to the same decision pool. Each source is independently configurable, respects the search's applicable filters and contributes results only when the decision stage is applied. Setting its candidate count to zero disables it. Its additions never appear in fallback. A failing optional source is skipped and never fails the search; decision-model errors follow the all-or-nothing fallback rule above. Gathering candidates never changes stored memory knowledge or access evidence; returned memory hits follow the accounting rule above.

- Graph (default 3): adds memory items connected to query-relevant entities that ordinary recall did not already place in the pool.
- Docs (default 3): adds relevant sections of embedded product help and leaf command sections of the generated CLI reference (`CLI`), identified by document and section, with a pointer to the document or command help. It never reads or changes memory state.
- Sessions (default off): adds matching excerpts of past provider conversations with their session provenance. Source failure or expiry skips these candidates; their presence does not turn a conversation into stored memory.

Source admission policy: the owner admits and retains sources only when measurements show better answers while keeping recall fast. This policy governs source selection; enabling a source does not certify its benefit.

## Reversible hygiene

The Dreamer consolidates memory through bounded, reviewable changes. A merge archives the lesser item, preserves its history and links it to the keeper; a conflict records which item supersedes the other. The keeper may receive a revised body in the same operation. Nothing is hard-deleted. Pinned items, skills, rules and preferences cannot be merged away.

Restoring an archived item returns its prior kind and invalidates the relation created by its merge while preserving the relation's history. Body versions remain separately restorable: restoring the lesser item does not silently undo a keeper-body revision. Dreamer merges and stale archives have separate daily caps enforced by the host.

Areas can be folded into another name without changing item scope or content. Every moved item keeps an audited prior area and can be restored independently. System areas, equal names and empty sources are refused; previews are read-only. The Dreamer judges bounded structural name candidates by their sample content, leaves uncertain pairs alone, and makes at most three area merges per run with a diary summary.

Stale archival is off by default and has a read-only preview with item ids and reasons. An item qualifies only when it is older than a year and has had no direct access, update or history change, no relation change, and no touch through an active neighbour during that year. Pinned items, skills, rules, preferences, already archived items and high-importance knowledge are excluded. Archival records the reason and remains reversible.

The graph hub report provides bounded, deterministic cluster suggestions and existing topic anchors. Suggestions distinguish structural congestion from provenance and leave graph changes to explicit operations; the Dreamer does not have to invent a grouping from an unbounded graph dump.

Run completion and diary finalization are separate facts. If a deadline or blocking command-approval prompt ends a dreaming run, the host preserves the work already logged, marks unfinished stages as cut short and records partial finalization with its reason. It never upgrades interrupted work to success, and it preserves an already completed diary.

Hidden memory workers record execution failures and stop within a 45-minute ceiling, or a shorter configured deadline. Extraction retries are bounded and backed off; exhausted items remain failed with a visible reason until an explicit retry or a change to the source or launch cause. Restarting the app does not reset that budget. Activity groups repeated identical failures and labels age separately from duration. An orphan's duration ends at its last observed activity, or remains unknown when that evidence is absent.

```map
anchors:
  - core:memory
  - capability:memory-and-skills
  - capability:memory-decision-rerank
  - capability:memory-hygiene
```
