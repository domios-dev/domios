# CodeTerm Mem

## Memory Protocol — follow these rules at all times

1. **BEFORE RESPONDING** about any person, project, or past event — `codeterm mem search` FIRST. Never guess from vague memory. Wrong is worse than slow.
2. **IF UNSURE** about a fact (name, date, decision, config) — say "let me check" and search. Don't fabricate.
3. **FOR IDENTITY QUESTIONS** (who am I, who is the user, what's my name, what's this machine) — use `codeterm mem wake-up`. It returns L0 identity + L1 critical facts. Never guess mem_item ids like `preference_identity` — they follow slug rules and rarely match what you expect.
4. **NEVER GUESS IDS** — if you don't see an id from search results, don't try `codeterm mem get <guessed_id>`. Always search first, then `get` a real id.
5. **WHEN THE USER SAYS "remember"** — save immediately via `codeterm mem save`. A background worker handles area classification, entity extraction, graph linking, and dedup. You don't need to worry about schema.
6. **WHEN FACTS CHANGE** — save the new fact. Old version stays in history. If explicitly outdated, `codeterm mem delete` the old one.
7. **WHEN A SESSION ENDS** (context compaction) — the PreCompact hook auto-flushes conversation to mem. You don't need to manually save session summaries.
8. **NEVER add the current date to the body.** The `created_at` and `updated_at` columns are tracked automatically — duplicating them in the body pollutes vector search.

## How saving works

Just write what you know into the body naturally. Include who was mentioned, what was decided, why it matters — all in one text. A background worker will handle area classification, entity extraction, graph linking, and dedup. You don't need to worry about schema. Just save.

```bash
codeterm mem save --kind note --title "china trip" \
  "Planning a trip to China with Anya, first time for both. User mentioned during casual chat about vacation plans."
```

Body quality matters. The body gets chunked and embedded as vectors — include names, project references, technologies, and context. "fixed the bug" is useless. "Fixed JWT refresh 401 by adding kid header from JWKS endpoint, Auth0 requires it" is searchable.

## Saving files and documents

When the user asks to "remember this file" or "index this doc":

```bash
cat /path/to/doc.md | codeterm mem save --kind document \
  --source-path "/path/to/doc.md" \
  --title "descriptive title for search"
```

`--source-path` tells the system where the original lives. A background Librarian checks periodically: if the file changed on disk, content + vectors are automatically re-indexed. If deleted, the item is flagged, not removed.

## Commands

- `codeterm mem search "<query>"` — full-text search across every kind of item. Returns ranked results with id, kind, title, short summary. Default limit 5.
- `codeterm mem get <id>` — print the full body of one item.
- `codeterm mem recall "<query>"` — semantic-first search (vectors when enabled, FTS5 fallback). Use when exact words might not match.
- `codeterm mem save --kind <kind> --title "<short title>" "<detailed content>"` — capture something new.
- `codeterm mem update <id> --area "..." --body "..."` — update an existing item.
- `codeterm mem delete <id>` — remove a specific item (also cleans vectors and relations).
- `codeterm mem forget <area>` — batch delete everything in an area.
- `codeterm mem sync` — trigger file sync for all source-tracked items.
- `codeterm mem wake-up` — prints L0+L1 context (identity + top-N important items).

## What to save as what

| Kind | Use for | Example |
|------|---------|---------|
| `fact` | Verified decisions, configs, architecture choices | "PgBouncer with transaction mode, max_client_conn=400" |
| `learning` | Bug fixes, gotchas, things you figured out | "JWT kid header required by Auth0 even though RFC says optional" |
| `preference` | User preferences, style choices | "Always use vim keybindings, prefer Dracula theme" |
| `note` | Meeting notes, conversations, context | "Talked to Tima about audio engine architecture" |
| `skill` | How-to instructions, reusable procedures | "How to deploy to fly.io: run ./build.sh build then..." |
| `rule` | Hard constraints, things to never/always do | "Never commit .env files" |
| `peer` | Machine/service info | "Mac Mini runs Docker with HA, Mosquitto, a gateway" |
| `document` | Indexed files tracked on disk | Saved via `--source-path`, auto-synced by Librarian |

## Filters

Add `--kind <kind>` to narrow by type. Valid kinds: `skill`, `fact`, `note`, `conversation`, `script`, `peer`, `preference`, `daily`, `learning`, `rule`, `summary`, `document`.

Add `--area <tag>` to narrow by subject area (e.g. `auth`, `billing`, `deploy`, `home`, `automation`).

Add `--scope workspace` to restrict to the current workspace's mem_items (global items are always included by default).

Add `--limit <n>` to change the result count.

## Graph queries (temporal facts and relationships)

- `codeterm mem graph related --id <id>` — what's connected to this thing
- `codeterm mem graph create --id <id> --entity-type <Type> --scope global` — create entity
- `codeterm mem graph relate --from <id> --rel <rel> --to <id>` — create relation
- Add `--as-of <date>` for point-in-time queries; stale / invalidated facts are filtered automatically

Graph entities are auto-created by the extraction worker. Entity types: Person, Project, Tech, Machine. Relations are free-form verb-based predicates (works_on, uses, shipped, runs_on, etc).

## Escalation order when a search misses

1. **Try literal keywords first** — `codeterm mem search "home automation"`.
2. **If no clear winner, broaden semantically** — `codeterm mem recall "what can I use to control lights"`.
3. **Follow graph relationships** — `codeterm mem graph related --id skill_home_assistant`.

Do NOT load every skill upfront. Fetch specific bodies only after a search identifies what's relevant.

## Invariants you must respect

- **Persistent memory lives in mem, nowhere else.** You never save anything important to `~/.claude/projects/*/memory/` or provider-specific auto-memory — those vanish on provider switch.
- **Never edit `CLAUDE.md`, `AGENTS.md`, or `AGENT.md`** — those are auto-generated by CodeTerm on every start.
- **Never write skill files into `~/.claude/skills/` or `~/.agents/skills/`** — save as mem_item: `codeterm mem save --kind skill --title <name> "<skill body>"`.
- **Every `save` should be specific.** "did some work" is noise. "Fixed JWT refresh 401 by adding kid header from JWKS endpoint" is searchable.
- **When a task needs a skill you haven't loaded, search for it first.**

## System power (keepawake)

When the user asks to prevent sleep, use `codeterm keepawake` — never `caffeinate`, `powercfg`, or `systemd-inhibit` directly.

- `codeterm keepawake start --duration-secs 7200` — keep system awake for 2 hours (add `--display` to also prevent display sleep)
- `codeterm keepawake start` — indefinite (until explicit stop)
- `codeterm keepawake stop` — release
- `codeterm keepawake status` — current session
