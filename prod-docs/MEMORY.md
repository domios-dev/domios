# CodeTerm Memory System

Persistent memory for agents. Survives restarts, provider switches
(Claude ↔ Codex), and CodeTerm upgrades. Agent-agnostic.

## TL;DR

- **Storage** — one local database at `~/.codeterm/mem/knowledge.db` plus large bodies as files beside it
- **Search** — text and semantic matches combined; filters narrow results, and decision re-rank applies when enabled
- **Graph** — temporal entity-relation triples (people, projects; NO tech nodes)
- **L1 selection** — pinned items first, then ranked by importance, use, workspace and recency; budget defaults to 300 tokens (`[memory] l1_budget_tokens`)
- **Pins** — `mem pin <id>` / `mem unpin <id>` set `is_pinned_wakeup`; pinned items always load in L1 (subject to budget)
- **Workers** — hidden headless agents that extract entities, build graph, dedup; `mem worker spawn` for ad-hoc enrichment
- **Skills** — folder skills (`SKILL.md` + scripts) registered via `mem skill new/add`, indexed alongside other items
- **Librarian** — periodic file-sync daemon: re-vectorizes source-tracked items when their on-disk file changes, flags deletions
- **Dreaming** — nightly consolidation at 6 AM with 0..30 min delay-only jitter: four SQL phases (light, deep, normalize, calibration) run server-side in `POST /api/mem/dream`, then the LLM REM worker is spawned to handle structural cleanup + diary; the REM prompt is read-only with respect to calibration (sweep already ran); hygiene is reversible (merge archives, never deletes)
- **Lifecycle capture** — `SessionStart` tracks the active session; optional `PreCompact` export POSTs to the authenticated memory endpoint only after separate local-user consent
- **Protocol** — 7 behavioral rules injected into every agent session

## Mem vs past-session search

Two stores, two purposes — don't confuse them:

| Surface | Stored where | Asks | Used for |
|---|---|---|---|
| **mem** (`~/.codeterm/mem/knowledge.db`) | local database with text and vector indexes | "what did the agent learn / decide / save?" | Curated facts, learnings, skills, conversations the agent chose to keep |
| **sessions** (provider-owned files) | Claude JSONLs / Codex rollouts / OpenCode's sqlite store | "what did we talk about that one time?" | Raw conversation history; on-demand search via `codeterm sessions search`, atomic resume via `codeterm sessions resume` |

Mem is curated and indexed; sessions are raw and provider-owned. The two never share storage. See `codeterm docs API-STATE` for the session API.

## Storage layer

### Item fields

- `kind` — `fact` | `learning` | `note` | `conversation` | `skill` | `rule` | `preference` | `peer` | `daily` | `summary`
- `scope` — `global` | `workspace:<id>`; any other value is refused
- `area` — project/topic tag (`auth`, `billing`, `deploy`, `home`, `_dreams`)
- `importance` — 1-5; the main wake-up ranking term
- pinned (`mem pin <id>` / `mem unpin <id>`) — pinned items always load in wake-up, subject to the budget

`mem save` uses kind plus title as its identity. An unchanged normalized body increments `recurrence_count` and schedules no extraction. A changed body updates the existing row in place and adds the prior body to history. `--replace` also updates in place and preserves the row's uid and creation time.

Editing bundled memory preserves your version; pin changes are audited. Graph nodes are people and projects only, and relations are temporal, so point-in-time queries work.

## Search & retrieval

Three layers:

| Layer | What | When | Size |
|---|---|---|---|
| **L0 — Identity** | `~/.codeterm/general/identity.md` | Injected into every GA init | ~100 tok |
| **L1 — Wake-up** | Pinned items + ranked by score (see below) | `codeterm mem wake-up` (GA runs on init) | ~300 tok (config: `[memory] l1_budget_tokens`) |
| **L2 — Targeted** | `search --area X --kind Y` | On-demand when topic comes up | variable |
| **L3 — Hybrid** | text and semantic recall | `codeterm mem search` / `recall` | variable |

Search combines text and semantic matches; use filters to narrow results and decision re-rank when enabled.

### Decision re-rank

An optional second stage over `mem search`. Stage 1 is the unchanged hybrid recall, widened to a candidate pool; stage 2 has a registered decision adapter grade each candidate and re-sorts the hits by that absolute relevance. It is off by default (`[memory.decision_rerank].enabled`); `codeterm mem search --decision-rerank on|off` overrides the config for one request, and an absent flag leaves the decision to config. The legacy `--rerank` is exclusive with `--decision-rerank on` (the request is refused as invalid).

- **Hits.** An applied search adds a `relevance` fraction (0..1) to each hit, shown as `rel 92%` next to the score in the human render.
- **Envelope.** `--json` returns the response unchanged plus a `decision_rerank` object: `status` is `applied` (with `adapter`, `low_confidence`, `max_relevance`), `fallback` (an adapter failure, deadline or missing adapter; plain recall hits are returned with a `reason`), or `skipped` (`reason: legacy_rerank`). The human render prints one line for fallback/skipped with the reason. Without the key the output is exactly the plain recall render.
- **Graph source.** `[memory.decision_rerank.sources]` sets the pool: `recall` hits (default 30) plus up to `graph` extra candidates (default 3; `0` turns the graph source off). Graph candidates are live mem items linked by an active relation to an entity whose id contains a query keyword, ranked by how many matched entities they link to, then importance and recency; they are found by SQL alone, never by a model or embedding. Items already in the recall pool are not added twice. With `graph > 0` an applied search tags every hit `via: "recall"|"graph"` and the envelope adds `sources: {recall, graph}` (pool counts). With both `graph = 0` and `docs = 0`, applied output carries neither key; every fallback also carries neither and matches the plain recall contract.
- **Docs source.** `[memory.decision_rerank.sources].docs` adds up to 10 embedded documentation sections (default 3; `0` turns the docs source off), selected by keyword match. It includes the embedded docs (`codeterm docs`) and the CLI reference as `CLI`, chunked by command heading. `memory.decision_rerank.docs_hybrid` defaults to `false`; when enabled and cached vectors are ready, the docs source fuses keyword and semantic matches. Missing vectors or a query embedding error leaves the keyword ordering unchanged. Applied hits use `{source: "doc", doc, section, anchor, pointer, relevance, via: "docs"}`; embedded docs point to `codeterm docs <NAME>` and CLI sections point to `codeterm <command> --help`. The envelope's `sources` counts include `docs`. The index never reads memory items or records access. Docs candidates appear only in applied output; off and fallback results retain the plain recall contract.
- **Sessions source.** `[memory.decision_rerank.sources].sessions` adds up to five recent matching snippets (default 0; `0` disables the source), using the same in-process provider search as `codeterm sessions search`. The snippet sent to the adapter contains its title, provider, date, and matched excerpt, capped by `max_body_chars`. Search runs under the rerank deadline and skips the source on error, timeout, or worker saturation. Applied hits use `{source: "session", session_id, provider, title, excerpt, pointer, relevance, via: "sessions"}` and point to `codeterm sessions get <id>`. Session and docs hits never record mem access. They appear only in applied output; off and fallback results retain the plain recall contract.
- **Access.** Pool candidates, recall or graph, record no access while they are graded. Only the hits the search returns count as accessed, once each.
- **Low confidence.** `low_confidence: true` means even the best hit scored below `low_confidence_threshold`: nothing relevant was found. The human render says so in one line; treat the hits as weak leads, not answers.
- **Token.** Adapter tokens never enter `config.toml` or argv. `printf %s "$TOKEN" | codeterm mem decision token set --adapter typesafe-jev` (or `pbpaste | ...`) reads stdin only and refuses a terminal; `token clear`, `token status [--adapter]` and `status` show presence, path and `mode_ok`, never the value. The token lives in `<data dir>/decision-models/<adapter>.json`: Unix uses a `0600` file in a `0700` directory, and Windows uses protected DACLs granting only the current user full control on the file and directory. Each instance keeps its own: `~/.codeterm/` for prod, `~/.codeterm-dev/` for dev. These commands use bearer HTTP on the local machine and never cross `--mesh`.

Keys and bounds: `codeterm docs RUNTIME-CONFIG`. Routes: `codeterm docs API-MEMORY`.

### Supporter Agent

The user-facing name of the decision model behind decision re-rank. It grades, it never acts; the default adapter is TypeSafe's Jev (`typesafe-jev`).

- **Where it works.** Memory re-rank (above): it re-orders recall and flags `low_confidence`. Tab state: shadow logging only, off by default; at the startup check it reads the tab's recent screen and writes one log line beside the structural verdict, changing no nudge or delivery.
- **Setup.** It needs only a token: Settings > Agents > Supporter Agent (a write-only password field; the status shows set / not set and warns when the file is not owner-only), or `pbpaste | codeterm mem decision token set`. Both use `/api/mem/decision/token` and the same `decision-models/` store. Re-rank itself is the toggle in Memory > Settings or `memory.decision_rerank.enabled`.
- **What is sent.** The query plus each candidate's title and body, bodies capped at `memory.decision_rerank.max_body_chars` (default 2000); for tab state, the tab's recent screen text.
- **Cost and latency.** About $0.0007 per search. The stage runs under `memory.decision_rerank.deadline_ms` (default 3000); a timeout, an adapter error or a missing token returns the plain recall hits with a `fallback` envelope.
- **Off.** `memory.decision_rerank.enabled = false`, `--decision-rerank off` for one search, or `codeterm mem decision token clear` to remove the token.
- **Links.** https://docs.typesafe.ai and https://console.typesafe.ai/keys.

### Wake-up selection

Wake-up includes global and selected-workspace memory; pins take precedence within the budget, overflow warns you to unpin or raise it, and `--explain` shows selection scores. `codeterm mem wake-up` inherits `CODETERM_WORKSPACE_ID` from the calling tab; `--workspace <ws>` explicitly overrides it, and without either only `scope = global` items participate.

## Graph layer

**Anti-star pattern:** only people and projects are entities. Technologies are searchable via FTS in body text — no graph nodes. This avoids the cascade where popular tech (React, Postgres) become mega-hubs.

**Canonical names:** dreaming worker merges name variants (Tima/Timofey → `person_tima`).

**Temporal validity:** relations have `valid_from`/`valid_to`. Dreaming's deep phase expires stale relations rather than deleting them.

**API:**
- `codeterm mem graph create --id <id> --entity-type Person --scope global` (idempotent)
- `codeterm mem graph relate --from <id> --rel mentions --to person_tima --reason "dreamer-bridge"`
- `codeterm mem graph related --id <id>` — outgoing relations

### Self-repair primitives

The Dreamer cleans historical pollution (duplicate relations, near-duplicate entities, orphan vectors) via typed primitives only — no raw SQL surface for the LLM. Every destructive op writes a full payload to `graph_purge_log` for reversibility.

| CLI | What it does | Audit-logged |
|---|---|---|
| `mem graph find-duplicate-relations --group-by …` | Read-only: list groups by whitelisted columns | no |
| `mem graph delete-relations --rowids 1,2,3` | Delete explicit rowids | yes (payload preserved) |
| `mem graph merge-relations --keep R --merge R1,R2` | Fold rows into `keep`, max(confidence), merge properties (canonical wins) | yes |
| `mem graph find-duplicate-entities --strategy exact-name\|alias-chain\|vector --threshold 0.85 --entity-type Tech` | Surface candidates. Vector strategy uses cosine-sim ≥ threshold | no |
| `mem graph alias --alias A --canonical C` | Merge entity A into C: rewire all relations, fold A's properties into C, delete A + its mem_vectors | yes (since #22) |
| `mem graph augment-entity --id E --patch '{"city":"Riga"}'` | PATCH entity properties + re-embed | yes (count=0) |
| `mem graph delete-entity --id E [--force] [--purge-history]` | Direct entity delete with safety net (errors if active relations and no `--force`). `--purge-history` also drops invalidated relations | yes |
| `mem graph repair-orphan-vectors [--dry-run]` | Sweep `mem_vectors` rows whose mem_id has no matching entity/mem_item | yes |
| `mem graph prune-thin-vectors [--dry-run]` | Delete vectors whose mem_item has an empty/title-only body (below the embed threshold) — the bodyless embeddings. Never touches a real-body vector or the item itself | yes |
| `mem graph collapse-synonym-edges [--dry-run]` | One-pass: per node-pair keep ONE membership predicate (about>applies_to>belongs_to>part_of>documented_in), invalidate the rest. Distinct predicates (uses, runs_on, …) untouched. Deflates hubs without hundreds of agent round-trips | yes |
| `mem graph prune-dream-hub-edges [--dry-run]` | One-pass: invalidate every dream-diary (kind=daily) → entity backlink via a noise predicate (about/applies_to/works_on/analyzes/…). Keeps the dreams anchor. Stops diaries inflating project/topic hubs | yes |
| `mem graph repair-orphan-blobs [--dry-run]` | CAS-aware sweep of `sha256_*.txt` blob files no `mem_item.body_blob_path` references | yes |

Use the bounded graph commands and their dry runs; do not issue raw SQL.

### Hub report

`mem graph hubs [--threshold 20] [--limit 5] [--sample 0]` is read-only and local. It returns at most `limit` nodes whose active degree is at or above `threshold`, highest degree first, as `{threshold, limit, sample, total_hubs, hubs, excluded, existing_anchors}`. `total_hubs` counts every hub before the limit; `excluded` lists the dreams anchor `project_dreams` (`{id, degree, reason}`), which is never a hub to decompose. Each hub is `{id, degree, rel_dist, popular_source, sub_anchors, outbound_only, unclustered, clusters, neighbors}`:

- `clusters` groups the hub's inbound neighbours (the edges `introduce-topic` can re-route) by shared area, then title keyword, then kind. A group must split the remaining neighbours and must not just repeat the hub's own name; existing `topic_*`/`role_*` neighbours count as `sub_anchors` instead. Each cluster is `{suggested_topic, entity_type, existing, basis, key, members}`: `suggested_topic` reuses an existing `topic_<key>` (`existing: true`) or names `topic_<hub-name>_<key>`, and the cluster feeds `introduce-topic --topic-id <suggested_topic> --type <entity_type> --sources <members> --anchor <hub>` directly.
- `popular_source` is true when every edge is `extracted_from` (provenance, no clusters).
- `neighbors` is the raw `{id, rel, dir, title}` sample, empty unless `--sample N`.

### Reversible hygiene

The Dreamer never hard-deletes duplicates or stale items; both end as `kind = 'archived'` through the calibration archive, with a history row that keeps the prior kind.

- `mem merge --keeper K --lesser L [--body B | --body-file F] [--conflict]` updates the keeper body, archives the lesser (history `merge`) and links `L merged_into K` (or `K supersedes L` with `--conflict`) in one transaction. Pinned, skill, rule and preference items are refused as the lesser. Dreamer merges are capped by `memory.dream_hygiene.merge_cap` (default 10 per UTC day), enforced server-side.
- `mem restore L` on an archived or merged item restores the prior kind (plus the `--version` body) and invalidates the merge relation.
- `mem calibration stale [--dry-run] [--limit N]` archives items older than 365 days with no direct touch (access, update, history row), no relation created or invalidated, and no touched active neighbour in that year. Pinned, skill, rule, preference, archived and importance >= 4 are excluded. The nightly run applies it only when `memory.dream_hygiene.stale_archive` is on (default off), capped by `stale_archive_cap` (default 20); the dry run lists ids and reasons and changes nothing.
- The diary log records `MERGED:` and `ARCHIVED:` lines with ids and reasons, and the worker report repeats them.

### Deadline finalization

A dreaming worker that hits its hard deadline is finalized by the host with finalization `partial` and reason `deadline`. A deadline-cut diary remains partial; complete diaries and a finalization the agent already recorded are preserved.

## Auto-flush from agent hooks

The mem flush API (`POST /api/mem/flush`) is the entry point providers call from their lifecycle hooks. There is no auth-exempt loopback path — every hook carries `Authorization: Bearer $CODETERM_API_TOKEN` (CodeTerm injects the token into every PTY at spawn). Claude's standard integration installs three hook handlers. `PreCompact` memory export is a separate opt-in because it exports conversation content:

| Hook | Endpoint | Purpose |
|---|---|---|
| `Notification` | `POST /api/tabs/{id}/hook/permission` | Surface a permission prompt without waiting for PTY auto-detect |
| `Stop` | `POST /api/tabs/{id}/hook/stop` | Mark task complete, fire `agent.task_complete` webhook |
| `PreCompact` (opt-in) | `POST /api/mem/flush` | Persist conversation summary to mem before Claude compacts the context window |
| `SessionStart` | `POST /api/tabs/{id}/hook/session_start` | Track the live `session_id` so `/new` and `/clear` inside the TUI don't desync the persisted resume id |

A flush obeys the same 1 MB body cap as `mem save`: scrollback beyond it keeps its most recent tail, and the item gets the `truncated` tag with the kept and original sizes in its summary. Indexing of a large item can stay pending without blocking smaller saves.

Hooks are installed through the provider integration action into `~/.claude/settings.json`. The action shows the target and proposed changes, records consent, makes a recoverable backup, and preserves foreign hooks.

## Related documents

Skills and background services: `codeterm docs MEMORY-SERVICES`. Agent behavior and command reference: `codeterm docs MEMORY-USAGE`.

<!-- revision: 30 -->
