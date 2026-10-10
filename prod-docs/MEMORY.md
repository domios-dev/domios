# Domios Memory System

Agent-agnostic persistent memory survives restarts, provider switches and upgrades.

## TL;DR

- **Storage** — one local database at `~/.codeterm/mem/knowledge.db` plus large bodies as files beside it
- **Search** — text and semantic matches combined; filters narrow results, and Supporter re-rank works automatically when connected
- **Graph** — temporal entity-relation triples (people, projects; NO tech nodes)
- **L1 selection** — pinned items first, then ranked by importance, use, workspace and recency; budget defaults to 300 tokens (`[memory] l1_budget_tokens`)
- **Pins** — `mem pin <id>` / `mem unpin <id>` set `is_pinned_wakeup`; pinned items always load in L1 (subject to budget)
- **Workers** — hidden headless agents that extract entities, build graph, dedup; `mem worker spawn` for ad-hoc enrichment
- **Skills** — folder skills (`SKILL.md` + scripts) registered via `mem skill new/add`, indexed alongside other items
- **Librarian** — periodic file-sync daemon: re-vectorizes source-tracked items when their on-disk file changes, flags deletions
- **Dreaming** — nightly consolidation at 6 AM with 0..30 min delay-only jitter: four SQL phases (light, deep, normalize, calibration) run server-side in `POST /api/mem/dream`, then the LLM REM worker is spawned to handle structural cleanup + diary; the REM prompt is read-only with respect to calibration (sweep already ran); hygiene is reversible (merge archives, never deletes)
- **Session lifecycle** — hooks bind sessions and report completion; terminal scrollback is never exported into memory.
- **Protocol** — 7 behavioral rules injected into every agent session

## Mem vs past-session search

Separate stores:

| Surface | Stored where | Asks | Used for |
|---|---|---|---|
| **mem** (`~/.codeterm/mem/knowledge.db`) | local database with text and vector indexes | "what did the agent learn / decide / save?" | Curated facts, learnings, skills, conversations the agent chose to keep |
| **sessions** (provider-owned files) | Claude JSONLs / Codex rollouts / OpenCode's sqlite store | "what did we talk about that one time?" | Raw conversation history; on-demand search via `codeterm sessions search`, atomic resume via `codeterm sessions resume` |

These stores never share storage. Session API: `codeterm docs API-STATE`.

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

Filters narrow hybrid search; a connected Supporter automatically re-ranks results.

### Supporter Agent memory re-rank

A second stage over `mem search` that runs automatically when an adapter is connected. Stage 1 remains hybrid recall, widened to a candidate pool; the adapter grades candidates and sorts them by relevance. Without a connected adapter, search behaves as before. There are no feature switches. `codeterm mem search --supporter on|off` is a per-request diagnostic override; without it, the connection decides. The legacy `--rerank` is exclusive with `--supporter on` (the request is refused as invalid).

- **Hits.** An applied search adds a `relevance` fraction (0..1) to each hit, shown as `rel 92%` next to the score in the human render.
- **JSON.** CLI `--json` hits contain only an item `id` (or a doc pointer), `source`, optional `via`, baseline `score`, and Supporter `relevance`; titles, bodies, summaries and tags are omitted. The Supporter envelope retains its status, fallback reason and low-confidence fields. Use `codeterm mem get <id> --json` for a full memory record. The HTTP search response keeps item details for the Memory UI.
- **Envelope.** The `supporter` object reports `applied` (with `adapter`, `low_confidence`, `max_relevance`), `fallback` (an adapter failure, deadline, missing adapter or exhausted budget; plain recall hits are returned with a `reason`, including `budget`), or `skipped` (`reason: legacy_rerank`). The human render prints one line for fallback/skipped with the reason. Without the key the output is exactly the plain recall render.
- **Graph source.** `[supporter.memory_rerank.sources]` sets the pool: `recall` hits (default 30) plus up to `graph` extra candidates (default 3; `0` turns the graph source off). Graph candidates are live mem items linked by an active relation to an entity whose id contains a query keyword, ranked by how many matched entities they link to, then importance and recency; they are found by SQL alone, never by a model or embedding. Items already in the recall pool are not added twice. With `graph > 0` an applied search tags every hit `via: "recall"|"graph"` and the envelope adds `sources: {recall, graph}` (pool counts). With both `graph = 0` and `docs = 0`, applied output carries neither key; every fallback also carries neither and matches the plain recall contract.
- **Docs source.** `[supporter.memory_rerank.sources].docs` adds up to 10 embedded documentation sections (default 3; `0` turns the docs source off), selected by keyword match. It includes the embedded docs (`codeterm docs`) and the CLI reference as `CLI`, chunked by command heading. `supporter.memory_rerank.docs_hybrid` defaults to `false`; when enabled and cached vectors are ready, the docs source fuses keyword and semantic matches. Missing vectors or a query embedding error leaves the keyword ordering unchanged. Applied hits use `{source: "doc", doc, section, anchor, pointer, relevance, via: "docs"}`; embedded docs point to `codeterm docs <NAME>` and CLI sections point to `codeterm <command> --help`. The envelope's `sources` counts include `docs`. The index never reads memory items or records access. Docs candidates appear only in applied output; off and fallback results retain the plain recall contract.
- **Sessions source.** `[supporter.memory_rerank.sources].sessions` adds up to five recent matching snippets (default 0; `0` disables the source), using the same in-process provider search as `codeterm sessions search`. The snippet sent to the adapter contains its title, provider, date, and matched excerpt, capped by `max_body_chars`. Search runs under the rerank deadline and skips the source on error, timeout, or worker saturation. Applied hits use `{source: "session", session_id, provider, title, excerpt, pointer, relevance, via: "sessions"}` and point to `codeterm sessions get <id>`. Session and docs hits never record mem access. They appear only in applied output; off and fallback results retain the plain recall contract.
- **Access.** Pool candidates, recall or graph, record no access while they are graded. Only the hits the search returns count as accessed, once each.
- **Low confidence.** `low_confidence: true` means even the best hit scored below `low_confidence_threshold`: nothing relevant was found. The human render says so in one line; treat the hits as weak leads, not answers.
- **Token mode.** Each adapter declares `required`, `optional`, or `none`. TypeSafe Jev requires its token. In plugin manifests, legacy bare `"decisionModel": true` means optional; `{ "tokenMode": "required" | "optional" | "none" }` is the explicit form. A selected, registered optional or none adapter connects without a token. A required adapter connects with a usable stored token. Connected status is `active`, or `budget_reached` when its shared call budget is exhausted; no selected adapter reports `not_connected`. Token-related states explain why a required adapter cannot connect.
- **Token.** Adapter tokens never enter `config.toml` or argv. `printf %s "$TOKEN" | codeterm supporter token set` (or `pbpaste | ...`) reads stdin only and refuses a terminal; `token clear`, `token status [--adapter]` and `status` show `token_mode`, presence, path and `mode_ok`, never the value. A none-mode adapter reports that it needs no token and ignores token writes. The token lives in `<data dir>/supporter/<adapter>.json`: Unix uses a `0600` file in a `0700` directory, and Windows uses protected DACLs granting only the current user full control on the file and directory. Each instance keeps its own: `~/.codeterm/` for prod, `~/.codeterm-dev/` for dev. These commands use bearer HTTP on the local machine and never cross `--mesh`.

Decision adapters target deterministic models such as [TypeSafe Jev](https://typesafe.ai). Fine-tunes and general LLMs may differ in accuracy and latency. Calibrated choice/noul probabilities describe decisions; sampled-text logprobs describe generated tokens and are not equivalent.

The Guide's local-model run missed all 80 deadlines; that evidence applies to the measured model. Re-rank stays on Jev. Shared-state requests need fresh quality and latency comparisons before shipping.

The adapter, model and token are managed in **Settings > Agents > Supporter Agent** or with `codeterm supporter`.

Keys and bounds: `codeterm docs RUNTIME-CONFIG`. Routes: `codeterm docs API-MEMORY`.

### Supporter Agent

The Supporter re-ranks memory, observes tab lifecycle events and supports bounded General Agent recovery. Connection enables it automatically; disconnected behavior is unchanged.

- **Connect.** Use Connect another adapter in Settings > Agents, choose its model and supply a token according to its declared token mode. The Connect & make active action keeps previous connections saved. For CLI setup, discover adapters with `codeterm supporter models`; save a required token first with `pbpaste | codeterm supporter token set --adapter <adapter>`, then run `codeterm supporter use <adapter> [--model <id>]`. Optional and token-free adapters connect without a token. Each saved row has its own remembered model and Make active action. `codeterm supporter disconnect <adapter>` removes that connection and token; removing active stops the Supporter without choosing another. Check `codeterm supporter status` for connection, activity, verdicts and budget.
- **Budget and fallback.** Requests have no cap by default. Optional `supporter.calls_per_hour` accepts 0 to 100000; absent or 0 means unlimited. Every physical HTTP attempt is counted and journaled; local decisions cost no request. Batched shared-state questions use one request; sequential adapters may use one per question. Per-candidate mode uses one per candidate. Only an explicit positive cap refuses calls at the limit: search returns baseline hits with reason `budget`, observation is skipped, and status shows `budget_reached` and the next free time. Unlimited status reports `calls_per_hour: null` with live hourly and daily counts. Adapter failures also return normal results. The stage deadline defaults to 3000 ms.
- **What is sent.** Memory queries and candidate titles/bodies, capped at `supporter.memory_rerank.max_body_chars` (default 2000), plus matching docs or session excerpts if configured. Tab observations send recent screen or chat text and runtime context. Jev receives this at TypeSafe; a local adapter sends it to its configured server. Tokens stay in protected local storage.
- **Prompt answers.** On agent-, Team-, worker- and cron-owned tabs, one calibrated Supporter choice at 0.80 or above can answer folder-trust, tool-permission and update prompts by selecting a provider-declared option through the existing response surface. Missing, failed, capped, `other` or low-confidence answers fall back to the structural responder; unresolved prompts notify the owner. Folder trust at launch or in cron does not require host provenance for this choice. Direct owner-launched tabs keep owner-facing prompts. Login, authentication, unavailable-model and quota prompts are never answered automatically. Details: `codeterm docs TAB-LIFECYCLE`.
- **Recovery.** At most two automatic General Agent restarts per recovery episode, then a failure notification and no further automatic recovery until the owner acts. A confirmed crash resumes a verified session only if the provider is absent from that tab's process tree. Startup failure and an error loop each allow one attempt within the overall limit. Automatic model-authorized recovery remains notify-only until its separate accuracy qualification passes. Details: `codeterm docs AGENT-CONTROL`.
- **Models.** Jev remains the calibrated re-rank model, billed to the TypeSafe account. Configure local models through OpenCode with `codeterm model-provider`; its optional decision adapter declares no calibrated choice confidence. Local models use the local machine; their latency and generated-token probabilities differ from Jev's decision probabilities.
- **Spend.** Domios estimates today and month spend from each response's `usage` and adapter-declared prices; unpriced adapters have no estimate. HTTP 402 or explicit credit exhaustion sends one `failure` alert and uses deterministic results until a later request probe succeeds.
- **Activity.** The Mission Control feed shows recent decisions and verdicts. The bounded `<data dir>/supporter/journal.jsonl` records automatic actions, budget transitions, answer distributions, adapter-reported model identities and criteria hashes. Inputs are hashes, never raw screen text or tokens. No verdict for `other` triggers an action or hint.
- **Diagnostic override.** `codeterm mem search --supporter off` skips re-rank for one search. Clearing a required token makes the active adapter unusable but keeps its saved connection; optional and token-free adapters remain connected without it.

The in-app Guide opens from a standalone `@ds-guide supporter-agent` paragraph. More about Jev: https://docs.typesafe.ai.

### Wake-up selection

Wake-up includes global and selected-workspace memory; pins take precedence within the budget, overflow warns you to unpin or raise it, and `--explain` shows selection scores. `codeterm mem wake-up` inherits `CODETERM_WORKSPACE_ID` from the calling tab; `--workspace <ws>` explicitly overrides it, and without either only `scope = global` items participate.

## Graph layer

**Anti-star pattern:** only people and projects are entities. Search technologies through body FTS; no tech nodes or popularity hubs.

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
- `mem area merge <from> --into <to> [--dry-run]` moves all source items across scopes, changing only their area. Preview returns counts per kind without writing. Empty, missing, equal and underscore-prefixed areas are refused; a new destination renames an area. Each item records one `update` with `area_merge` undo data in its history reason. `mem restore <id>` undoes its latest area merge; `--version <history-row>` selects an older merge without changing its body. Use `mem taxonomy` to list areas and counts. REM receives up to 20 conservative name pairs with counts and three sample titles each; it leaves uncertain pairs alone. The host caps area merges at 3 per dream worker run and saves a summary diary entry for each.
- `mem restore L` on an archived or merged item restores the prior kind (plus the `--version` body) and invalidates the merge relation.
- `mem calibration stale [--dry-run] [--limit N]` archives items older than 365 days with no direct touch (access, update, history row), no relation created or invalidated, and no touched active neighbour in that year. Pinned, skill, rule, preference, archived and importance >= 4 are excluded. The nightly run applies it only when `memory.dream_hygiene.stale_archive` is on (default off), capped by `stale_archive_cap` (default 20); the dry run lists ids and reasons and changes nothing.
- The diary log records `MERGED:` and `ARCHIVED:` lines with ids and reasons, and the worker report repeats them.

### Deadline finalization

A dreaming worker that hits its hard deadline is finalized by the host with finalization `partial` and reason `deadline`. A deadline-cut diary remains partial; complete diaries and a finalization the agent already recorded are preserved.

## Session hooks and memory

Claude hooks bind the current session and report permission/completion events. Raw terminal export has been removed: compaction does not create memory items. Consented integration repair removes only the retired Domios-owned PreCompact exporter and preserves foreign hooks.

| Hook | Endpoint | Purpose |
|---|---|---|
| `Notification` | `POST /api/tabs/{id}/hook/permission` | Surface a permission prompt without waiting for PTY auto-detect |
| `Stop` | `POST /api/tabs/{id}/hook/stop` | Mark task complete, fire `agent.task_complete` webhook |
| `SessionStart` | `POST /api/tabs/{id}/hook/session_start` | Track the live `session_id` so `/new` and `/clear` inside the TUI don't desync the persisted resume id |

Explicit `mem save` remains available with its 1 MB body cap. Provider transcripts remain in the provider session store.

Hooks are installed through the provider integration action into `~/.claude/settings.json`. The action shows the target and proposed changes, records consent, makes a recoverable backup, and preserves foreign hooks.

## Related documents

Skills and background services: `codeterm docs MEMORY-SERVICES`. Agent behavior and command reference: `codeterm docs MEMORY-USAGE`.

## Retired raw-export cleanup

After updating the owning peer's daemon and CLI, preview an already verified and deleted record locally with `codeterm mem purge-precompact <id>`. Apply with `--apply --verified-deleted`; `--vacuum` compacts FTS/SQLite and requests current-WAL truncation.

The prior deletion receipt must establish conversation kind, `flush:<tab> pre-compact` title, matching tab/pane source, PTY scrollback summary and flush/conversation tags. History does not retain source/summary/tags: `--verified-deleted` attests that prior receipt, not newly reconstructed provenance. The command refuses live, renamed, promoted, merged, unknown or oversized records. It purges only their history, exact vectors/FTS rows and attributable unshared body blobs. Shared blobs and graph knowledge remain. A payload-free receipt supports retry after file failure; partial cleanup is an RPC error with its report.

Check row/byte counts and compaction outcomes. Blob unlink, VACUUM and WAL checkpoint reclaim storage, without secure erasure. Backups, snapshots, old WAL copies, filesystem free space and provider transcripts remain untouched. No global orphan sweep runs.

<!-- revision: 46 -->
