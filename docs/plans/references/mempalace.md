# Reference: MemPalace

**Source:** https://github.com/milla-jovovich/mempalace
**License:** MIT
**Imported:** 2026-04-08
**Relevant to:** `docs/plans/memory-architecture.md` — CodeTerm memory plugin (Phase 3)

## What it is

Local, open-source AI memory system for LLM conversations. Stores verbatim rather than summarizing. Headline number: **96.6% on LongMemEval** (raw mode). Everything runs on the user's machine; no cloud.

## Architecture in one paragraph

ChromaDB for semantic vector search + SQLite for temporal knowledge-graph triples. Raw verbatim text is the default storage form (not summarized). An experimental "AAAK" abbreviation layer exists but currently regresses performance (84.2% R@5 vs 96.6% raw) and isn't the default. The defining idea is a hierarchical navigation metaphor called "The Palace" that replaces flat unstructured search with structured routing.

## The Palace metaphor

The storage is organized as a navigable spatial structure borrowed from ancient memory techniques:

| Layer | What it represents | CodeTerm analogue |
|---|---|---|
| **Wings** | Top-level people or projects | workspace-level scope (we already have this) |
| **Rooms** | Topic areas within a wing (auth, billing, deployment, ...) | feature / area within a workspace |
| **Halls** | Memory-type corridors inside a wing (facts, events, discoveries, preferences, advice) | memory type tagging (we planned kind: fact/event/rule/summary) |
| **Tunnels** | Cross-references between rooms across wings | graph relations (we planned this in knowledge.db) |
| **Closets** | Summary pointers that link back to original content | facts/{id}.md files + SQLite metadata (we planned this) |
| **Drawers** | Original verbatim files, never summarized | we were planning to summarize via dreaming — see tradeoff below |

**Retrieval win claim:** searching with `wing + room` context achieves **94.8% R@10** vs **60.9%** for flat search over all closets. That's a 34% boost from structure alone — metadata filtering, not a new vector algorithm.

Caveat from the project's own published correction: the "34% palace boost" conflates standard ChromaDB metadata filtering with novel retrieval. It's honest — the win comes from filtering, which any store with metadata can do. Our SQLite + FTS5 can do the equivalent with `WHERE workspace_id = ... AND area = ...`.

## Memory stack (layered loading)

| Layer | Size | When loaded |
|---|---|---|
| **L0 — Identity** | ~50 tokens | Always in system prompt |
| **L1 — Critical facts** (team, projects, preferences) | ~120 tokens (AAAK compressed) | Always in system prompt |
| **L2 — Room recall** | variable | On demand when the conversation touches a known room |
| **L3 — Deep semantic search** | variable | On demand for fuzzy queries |

`mempalace wake-up` returns ~170 tokens of critical context designed to be injected into a system prompt at session start. This maps cleanly onto our GA `init` kick pattern — the GA reads `MEMORY.md` on init, which could be the L0+L1 equivalent.

## Storage model details

- **ChromaDB** for vector search (local, no cloud)
- **SQLite** for temporal knowledge graph triples with validity windows (`entity relation entity, valid_from, valid_to`)
- **Raw verbatim files** as primary storage (Drawers)
- **Three mining modes** the user can trigger: `projects` (code/docs), `convos` (conversation exports), `general` (auto-classified facts/events/problems)

## AAAK — the experimental failed direction

AAAK = abbreviation dialect with entity codes, structural markers, sentence truncation. Pitched as "30× lossless compression" originally but the project published a correction: it's **lossy abbreviation**, not compression, and at small scales it **doesn't save tokens** (example: real token counts 73 vs 66 — worse, not better). Designed for repeated entities across thousands of sessions, not general use.

**Takeaway for us:** do not go down the custom dialect path. Use real compression (gzip) for cold storage, summaries for curated output, and raw verbatim for hot.

## Integration model

**MCP server** — `mempalace mcp_server` exposes 19 tools including `mempalace_search`. Claude / ChatGPT / Cursor / Gemini call it automatically via MCP.

**CLI** — `mempalace search "query"` with optional wing/room filters. `mempalace wake-up` prints the L0+L1 context for pipeline use (`claude -p "$(mempalace wake-up)\n\n<user message>"` pattern).

**Python API** — `search_memories(...)` function.

## What to adopt for CodeTerm memory plugin

| Idea | Adopt? | Rationale |
|---|---|---|
| **Verbatim-first storage** | **Yes** — align with our existing raw PTY flush + daily notes. MemPalace's data shows summarization hurts retrieval more than people expect. Dreaming stays, but dreaming writes summaries ALONGSIDE verbatim, never replacing it. | |
| **Hierarchical scope filtering** (wing/room = workspace/area) | **Yes (as metadata tags)** — we already have workspace scope. Add a nullable `area` tag to memory items, let queries filter by `(workspace_id, area)` via SQL. Do NOT introduce a new "palace" vocabulary; "workspace + area" is already our model. | |
| **Temporal validity windows** on graph facts | **Yes** — our graph is already SQLite-backed. Add `valid_from TEXT` and `valid_to TEXT NULL` columns to the `relations` table. Cheap, enables "as of <date>" queries, and prevents stale facts from polluting recall. | |
| **Layered memory stack (L0–L3)** | **Yes (already partially there)** — our plan has MEMORY.md (L0+L1 curated), facts/{id}.md files (L2), FTS5+vectors (L3). Rename the layering for clarity in the plan doc. | |
| **`wake-up` CLI command** | **Yes — map to our existing GA init flow** — `codeterm memory wake-up` prints the top-N critical items as a short prompt-ready block. The GA `init` kick becomes `codeterm memory wake-up && <eval MEMORY.md>`. | |
| **ChromaDB for vector search** | **No** — stick with `sqlite-vec` per our current plan. ChromaDB adds a second storage engine; our plan explicitly picks SQLite-only for the "one local file, easy to back up" property. | |
| **AAAK dialect** | **No** — the project's own correction says it regresses at small scale. Custom dialects are maintenance burden with no proven win. Use real compression for cold storage if we ever need it. | |
| **Raw verbatim-only (no consolidation)** | **Partial** — we keep dreaming (Light/Deep/REM) per the current plan because our use case includes pre-compaction flush and cross-session knowledge consolidation. But dreaming writes ALONGSIDE verbatim, not replacing it. MemPalace shows verbatim retrieval is strong enough that we shouldn't throw the raw away. | |
| **MCP server exposing memory tools** | **Partial** — our plan has `codeterm memory *` CLI as the primary interface (CLI + Skills > MCP, per our plugin-architecture.md decision). MCP wrapper stays optional for ecosystem discoverability. | |
| **Mining modes** (projects / convos / general) | **Maybe later** — interesting UX for one-shot batch imports, but not core to the always-on memory use case. Could surface as `codeterm memory import <mode> <path>` if we want it. | |

## Honest status of the project (as of April 2026)

Published corrections from the maintainers:
- AAAK token savings claim was incorrect (real counts: 73 vs 66 — worse)
- "30× lossless compression" overstated; it's lossy abbreviation
- "34% palace boost" conflates metadata filtering (standard ChromaDB) with novel retrieval
- Contradiction detection exists but isn't wired into knowledge graph operations yet
- "100% with Haiku rerank" is real but rerank pipeline isn't in public benchmark scripts
- 96.6% headline number is **raw mode specifically**, not AAAK or Rooms mode

This is a project worth learning from, not copying. The organizing ideas are good; the specific engineering choices (AAAK, ChromaDB) are either experimental failures or arbitrary.

## Files visible in the repo (for future deep dives)

```
.github/
assets/
benchmarks/       ← LongMemEval runner lives here
examples/
hooks/
mempalace/        ← main Python package
tests/
.gitignore
.pre-commit-config.yaml
CONTRIBUTING.md
LICENSE
README.md
pyproject.toml
uv.lock
```

Worth reading if we implement Phase 3:
- `mempalace/` — storage layer, graph implementation, MCP server
- `benchmarks/` — LongMemEval harness; might be reusable as an eval set for our own memory plugin the same way `evals/general-agent/evals.json` drives prompt-evaluator
- `hooks/` — pre-commit hooks, irrelevant to us
- `examples/` — integration samples, might inform our CLI UX

## What to NOT carry over

- **The palace vocabulary** as-is. "Wings", "halls", "drawers", "closets" is evocative but it's a UX decision, not an architecture one. Our vocabulary is already `workspace / fact / relation / daily / learning / dreaming` which matches our existing codebase and plan. Don't rename everything to fit a metaphor.
- **Verbatim-only purism.** MemPalace argues verbatim beats summaries on retrieval — fine, we'll keep verbatim. But we also want consolidated summaries for the GA system prompt (L0+L1 budget) and for cross-session narrative continuity. Dreaming stays.
- **AAAK.** See above.
- **Second database engine** (ChromaDB). Our plan is SQLite-only for backup / sync / simplicity. sqlite-vec is the vector option when we need it.

## Action items for the memory plan

Track these as concrete updates to `docs/plans/memory-architecture.md` when Phase 3 starts:

- [ ] Add `area TEXT NULL` column to `memory_items` (maps to MemPalace "rooms"); use for metadata-filtered search
- [ ] Add `valid_from TEXT NOT NULL` and `valid_to TEXT NULL` columns to `relations`; implement `WHERE valid_from <= :as_of AND (valid_to IS NULL OR valid_to > :as_of)` for as-of queries
- [ ] Add `codeterm memory wake-up` CLI command — prints top-N critical items in ~170-token budget, designed for injection into GA system prompt at init
- [ ] Add a "Memory stack layers" section to the plan doc with explicit L0/L1/L2/L3 budgets (L0 ~50, L1 ~120 compressed, L2 on-demand room recall, L3 full semantic search)
- [ ] Evaluate whether we can reuse MemPalace's LongMemEval runner as an eval set for `codeterm memory recall` — same pattern as `evals/general-agent/` + `prompt-evaluator` skill

---

_This reference is imported for research purposes. It's not a dependency, not vendored code, not a template we're following. It's one data point in the memory-system design space that we're building CodeTerm's own implementation for._
