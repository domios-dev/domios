# Reference: mem0

**Source:** https://github.com/mem0ai/mem0
**License:** Apache-2.0
**Imported:** 2026-05-07
**Local clone:** removed after analysis (re-clone to verify citations)
**Relevant to:** [`docs/plans/mem-optimization.md`](../mem-optimization.md) — Tracks A (extraction), B (retrieval), C (graph maintenance)

## What it is

The most popular AI-agent memory library on GitHub. Python, ~30k+ stars, Apache-2.0. Pluggable across 28+ vector store backends, OpenAI-default embeddings, LLM-driven extraction with hash-based deduplication. Sync + async API.

## Architecture in one paragraph

`Memory` / `AsyncMemory` (`mem0/memory/main.py:331,1795`) is the entry-point class. `add()` runs an LLM extraction pass (`ADDITIVE_EXTRACTION_PROMPT` in `mem0/configs/prompts.py`), emits a list of facts as JSON `{memory: [{text, linked_memory_ids}]}`, MD5-hashes each for dedup, and stores them as vectors in a pluggable backend (default Qdrant). A separate "entity store" — its own vector collection named `{collection}_entities` — holds extracted entity names for query-time entity-boost. Search runs semantic vector search + BM25 (when backend supports it) + entity-boost scoring with named weights, optional reranker. Memory categories (`SEMANTIC`/`EPISODIC`/`PROCEDURAL`) exist as enum values but only `PROCEDURAL` has a distinct code path; the other two are stored in the same vector store with a metadata tag.

## Specific findings

### Top-level API

Verbs: `add`, `search`, `update`, `delete`/`delete_all`, `get`/`get_all`, `history`. Both sync and async.

`add()` requires at least one of `user_id`, `agent_id`, `run_id` (`mem0/memory/main.py:301-307`). Hard error otherwise. All memories tagged with these IDs as metadata for filter-based isolation.

### Extraction pipeline

When `add("the user prefers Vim", user_id="u1")` runs with `infer=True`:

1. **Phase 0:** query existing memories (top-10 by similarity, lemmatized) for context.
2. **Phase 1-2:** LLM call with `ADDITIVE_EXTRACTION_PROMPT` (`mem0/configs/prompts.py`). Existing memories passed as 0-indexed list (NOT UUIDs — explicitly to prevent UUID hallucination). Output JSON: `{"memory": [{"text": "...", "linked_memory_ids": [<idx>, ...]}]}`.
3. **Phase 3:** MD5 hash each extracted text; skip if hash exists (`mem0/memory/main.py:~880`).
4. **Phase 4:** batch-embed all new texts.
5. **Phase 5-7:** insert into vector store with metadata payload; extract entities via spaCy (`mem0/utils/entity_extraction.py`) and upsert to `{collection}_entities`.

**No "new fact contradicts existing fact → invalidate" logic.** Just hash-dedup and `linked_memory_ids` (LLM's own grouping decision).

### Hybrid search

`mem0/memory/main.py:1327-1430`:

1. Semantic over-fetch (4× `top_k`) from vector store.
2. BM25 keyword search (if backend supports `keyword_search()`).
3. Entity boost: extract entities from query, boost results containing them.
4. Score fusion (`mem0/utils/scoring.py`): normalized semantic similarity + sigmoid-normalized BM25 + binary entity boost. Tunable weights (e.g., `ENTITY_BOOST_WEIGHT = 0.15`).
5. Optional reranker.
6. Metadata filters: `eq`, `ne`, `in`, `gt`, `contains`, logical `AND`/`OR`/`NOT`.

Defaults: `top_k=20`, `threshold=0.1`.

### Entity store

Separate vector collection just for entity names (`mem0/memory/main.py:390-411`). `linked_memory_ids` on each entity points back to the memory items mentioning it. Used for entity-boosted search ranking, not for graph traversal — there are no edges between entities.

## Why it's relevant to codeterm mem

**Three takeaways worth lifting into our plan:**

1. **`linked_memory_ids` in the extraction output.** When the auto:save LLM worker extracts a new fact, it should also be allowed to declare "this groups with existing facts X and Y" via id references. Avoids the post-hoc cosine merge for cases the model already understood at extraction time. → Track A2 (Pydantic-style structured extraction).

2. **Entity-name vectors as a secondary index.** Embed each entity's canonical name (1.5KB × ~150 entities = 230KB, trivial). Enables cosine-based fuzzy matching for entity merging during dream consolidation. Solves Open Question 3 in the main plan. → Track C2 (entity merge).

3. **Named-weight scoring instead of pure RRF.** mem0's `score_and_rank` (`mem0/utils/scoring.py`) fuses with explicit named weights (`semantic_weight`, `bm25_weight`, `entity_weight`). Easier to tune than RRF's opaque `1/(k + rank)`. Worth comparing on our 24-query doc bench when investigating the FTS-vs-vector imbalance. → Track B1.

## What's NOT applicable

- **OpenAI-default embeddings** (`mem0/embeddings/configs.py:9` defaults `provider="openai"`). We are local-first / fastembed-only. Don't import this default.
- **28+ pluggable vector backends.** We have one (SQLite + fastembed inline). The plugin abstraction adds cost without payoff at our scale.
- **Procedural-memory special path** (LLM summarization at ingest, `mem0/memory/main.py:1618-1653`). Their procedural memory is "summary of how the agent did a task." We model this as a regular `kind=skill` item with no special pipeline. Don't fork.
- **Mandatory `user_id`/`agent_id`/`run_id` scoping.** Single-user; we don't need a per-call multi-tenancy boundary.

## Code provenance

mem0/memory/main.py — top-level API
mem0/configs/prompts.py — extraction prompt
mem0/utils/scoring.py — hybrid scoring math
mem0/utils/entity_extraction.py — spaCy entity extraction
mem0/vector_stores/configs.py — backend list
