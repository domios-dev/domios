# Reference: Letta (formerly MemGPT)

**Source:** https://github.com/letta-ai/letta
**License:** Apache-2.0
**Imported:** 2026-05-07
**Local clone:** removed after analysis (re-clone to verify citations)
**Relevant to:** [`docs/plans/mem-optimization.md`](../mem-optimization.md) — Track A (extraction), Track D (stretch: agent-curated core memory)

## What it is

The academic-origin agent memory system that introduced "virtual context management" — paging memory in and out of an LLM's context window like an OS pages RAM in and out of a CPU. Now productized as Letta, the company. Apache-2.0, async-first Python, runs as a long-lived agent server. Postgres or SQLite + pgvector or Turbopuffer for hybrid search.

## Architecture in one paragraph

Three tiers: **core memory** (always in context, character-budgeted, agent-editable), **archival memory** (unbounded long-term passages, vector + FTS searchable), **recall memory** (every message ever, dual-indexed). Agents have first-class tools (`core_memory_replace`, `core_memory_append`, `archival_memory_insert`, `archival_memory_search`, plus newer `memory_apply_patch` for unified-diff style edits) that they call themselves to mutate their own memory. The defining idea is **agent-curated core memory blocks** — the agent edits its own context window via tool calls, treating it like a writable filesystem.

## Specific findings

### Three-tier memory model

Defined in `letta/constants.py`:

| Tier | Storage | Size budget | Access | Agent tools |
|---|---|---|---|---|
| **Core memory** | `blocks` table (Postgres/SQLite) | persona=20K chars, human=20K chars, custom=100K chars/block | Always in LLM context | `core_memory_replace`, `core_memory_append`, `memory_apply_patch` |
| **Archival memory** | `archival_passages` table + vector index | Unbounded | On-demand via search | `archival_memory_insert`, `archival_memory_search` |
| **Recall memory** | `messages` table + vector + FTS | Unbounded | Hybrid search | `conversation_search` |

`letta/orm/passage.py:L76-L105` defines `ArchivalPassage`: id, text, embedding (pgvector for Postgres, CommonVector for SQLite), tags (JSON + junction table), archive_id (FK).

### Core memory blocks

`letta/schemas/block.py:L67-L86` — `Block { value: str, limit: int, label: str, description, read_only: bool }`. `letta/schemas/memory.py:L68-L885` — `Memory` wraps `List[Block]`. `compile()` renders blocks into XML for the LLM prompt.

Special block types: `Human` and `Persona` subclasses with fixed labels. Agents can also create custom blocks (e.g., `system/context`, `current-task`).

### Agent-editable memory tools

`letta/services/tool_executor/core_tool_executor.py`:

- `core_memory_append(label, content)` (`L319-L326`) — appends text + newline.
- `core_memory_replace(label, old, new)` (`L328-L344`) — exact string replacement, raises if `old` not found.
- `memory_replace`, `memory_str_replace`, `memory_apply_patch` (`L346-L423`) — newer git-style tools with line numbers and unified-diff support.

All check `read_only` flag and call `agent_manager.update_memory_if_changed_async()` to persist.

### Hybrid recall

When agent calls `archival_memory_search(query, tags, top_k)`:

1. `build_agent_passage_query()` constructs SQL with optional embedding search and FTS.
2. Embedding-based via stored vectors (vector distance).
3. FTS via SQLite FTS5 or Postgres `tsquery`/`plainto_tsquery`.
4. **Hybrid via Turbopuffer** if configured: `search_mode="hybrid"` (`letta/services/agent_manager.py:L2463-L2466`) — combines vector + BM25 with RRF.
5. **Results carry debug annotations:** `rrf_score`, `vector_rank`, `fts_rank` on each result (`letta/services/agent_manager.py:L2621-L2625`).
6. Tag filtering (any/all mode), temporal bounds (`start_date`/`end_date`).

### Git-backed memory filesystem

`letta/schemas/memory.py:L205-L349` — agents with `enable_git_memory=True` get structured labels rendered as a directory tree:

```
system/persona
system/context
skills/<skill-name>
notes/<topic>
```

Line-numbered render. Patch tools operate on this view as if it were files — agent says "patch line 42 of `system/context` to add this paragraph."

### Identity model

`letta/schemas/identity.py:L44-L53` — `Identity { identifier_key, name, identity_type (user/org), properties }`. Agents have a `memory: Memory` field. Blocks are scoped per-agent. The `Human` block is *the agent's record of the human*, not a persistent user table — opposite of a user database.

## Why it's relevant to codeterm mem

**Three takeaways worth lifting:**

1. **Agent-curated "core memory blocks" as a stretch addition to mem.** Today our agents `mem save` everything as immutable items. A small set of always-in-context, agent-editable blocks (e.g., `current-task`, `persona`, `working-set`) that the agent itself maintains via `mem core append/replace/patch` would give us an upper tier of fast-mutation memory the agent owns. Different shape from cognee's two-tier (which is automatic LLM bridge); Letta's is explicit, agent-driven. → Could land in Track D2 (two-tier memory) or as a separate stretch track.

2. **Debug-annotated search results.** Letta returns `rrf_score`, `vector_rank`, `fts_rank` on every hit. We could add this with a `--debug` flag or always include it in `--json` output. Helps diagnose the documented FTS-vs-vector imbalance without instrumenting production code. → Tiny add to Track B.

3. **Git-backed memory filesystem.** Maps to our skill-folder workflow naturally. Today `mem skill new <slug>` scaffolds a folder with `SKILL.md` + `scripts/`. Extending to `mem memory new <label>` for agent-editable blocks (rendered as files, patched via tool calls, source-tracked by Librarian) is a clean composition. → Stretch, after core tracks land.

## What's NOT applicable

- **Long-lived agent server architecture.** Letta runs an HTTP server with persistent agent processes. We're CLI-driven, stateless per invocation. Memory mutations must be load → mutate → save, not async streaming.
- **Org-level multi-tenancy.** Letta has org-scoped isolation, team archives, shared memories. We're single-user, single-workspace.
- **Turbopuffer as second vector backend.** External hybrid-search service. We standardize on SQLite + fastembed in-process; no network calls for default ops.
- **Mandatory `agent_state`-bound memory.** Letta requires every memory to belong to an agent. Our memory belongs to a user, with optional `scope=workspace` — different ownership model.

## Code provenance

letta/constants.py — character budgets
letta/orm/passage.py — archival passage schema
letta/schemas/block.py — block model
letta/schemas/memory.py — memory class + git-backed filesystem
letta/services/tool_executor/core_tool_executor.py — agent memory tools
letta/services/agent_manager.py — search + retrieval pipeline
