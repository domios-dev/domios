# CodeTerm Mem

## Memory Protocol — follow these rules at all times

1. **BEFORE RESPONDING** about any person, project, or past event — `codeterm mem search` FIRST. Never guess from vague memory. Wrong is worse than slow.
2. **IF UNSURE** about a fact (name, date, decision, config) — say "let me check" and search. Don't fabricate.
3. **FOR IDENTITY QUESTIONS** (who am I, who is the user, what's my name, what's this machine) — use `codeterm mem wake-up`. It returns L0 identity + L1 critical facts. Never guess mem_item ids like `preference_identity` — they follow slug rules and rarely match what you expect.
4. **NEVER GUESS IDS** — if you don't see an id from search results, don't try `codeterm mem get <guessed_id>`. Always search first, then `get` a real id.
5. **WHEN THE USER SAYS "запомни" / "remember"** — save immediately via `codeterm mem save`. Extraction workers will auto-process the graph (people, projects, area classification). You don't need to create graph entities manually.
6. **WHEN FACTS CHANGE** — save the new fact. Old version stays in history. If explicitly outdated, `codeterm mem delete` the old one.
7. **WHEN A SESSION ENDS** (context compaction) — the PreCompact hook auto-flushes conversation to mem. You don't need to manually save session summaries.

## How memory works

There are two save paths:

### Inline structured save (preferred — you have context)

When YOU save something and you already know who/what/where it relates to, pass the structure inline. The save then skips the background extraction worker entirely — faster, no hallucinations, no race conditions:

```bash
codeterm mem save --kind fact \
  --area codeterm \
  --people andrey,tima \
  --projects codeterm \
  --tags worker_pool,fifo \
  --importance 3 \
  --title "worker pool FIFO cap at 3" \
  "Per-provider semaphore caps concurrent extraction workers at 3, queued FIFO."
```

`--people` and `--projects` accept comma-separated slugs ("andrey") or full ids ("person_owner"). They become `person_*` / `project_*` graph entities + `mentions` relations from the new item. Idempotent — re-using a name just links to the existing entity.

**Threshold for "structured":** any of `--area`, `--tags`, `--people`, `--projects` set. If you provide ANY of them, the extraction worker is skipped.

### Unstructured save (fallback)

If you don't pass structure (raw `codeterm mem save --kind X "body"`), an extraction worker spawns in the background, reads only the body, and tries to infer area/people/projects/decisions itself. **Slower, hallucination-prone path** — use only when you genuinely don't have context (scripts piping logs, raw user dictation, etc).

### Area decision table

Pick `--area` from the strongest signal in the body. If no signal matches, omit `--area`.

| Keywords in body | --area |
|---|---|
| codeterm, worker, cron, mem, knowledge, graph, dreaming | codeterm |
| soundcalm, binaura, audio engine, healthkit, sleep | soundcalm |
| rekap, langgraph, langchain, livekit, whisper | rekap |
| home assistant, mosquitto, router, tailscale, 4090, blackstar | home |
| auth, jwt, oauth, session, token, login | auth |
| deploy, docker, fly.io, vercel, ci, github actions | deploy |

## Graph rules — CRITICAL

The graph is ONLY for real-world entities: **people** and **projects**. Nothing else.

**NEVER create** category/type entities like `rule`, `preference`, `identity`, `kind_fact`, `tech_react`, etc. These are:
- `kind` is already a column on every mem_item — filter via `--kind preference`, no graph needed
- Categories create star patterns (1 node + N edges to every item of that kind)
- Technologies are searchable via FTS in body text — no graph node needed

**NEVER create** relations like `kind_of`, `belongs_to_category`, `is_a`. The `kind` column handles this.

Allowed entity ID prefixes: `person_*`, `project_*`. That's it. If an extraction worker finds them, great. You yourself should save items and let the workers build the graph.

## Commands

- `codeterm mem search "<query>"` — full-text search across every kind of item. Returns ranked results with id, kind, title, short summary. Default limit 5.
- `codeterm mem get <id>` — print the full body of one item. Use this to load a skill's instructions, the contents of a script, or the full text of a past conversation excerpt.
- `codeterm mem recall "<query>"` — semantic-first search (vectors when enabled, FTS5 fallback otherwise). Use when the exact words you'd type might not appear in the stored content.
- `codeterm mem save --kind <kind> --title "<short title>" "<detailed content>"` — capture something new. Use `--kind fact` for validated decisions, `--kind learning` for pending insights to promote later, `--kind note` for short context that might be useful again. **Always provide a descriptive --title and body** — the body (positional arg) is what gets vectorized for semantic search, so include enough detail. Area is auto-detected from content by extraction workers, but you can set `--area <project>` explicitly if you know it.
- `codeterm mem update <id> --area "..." --body "..."` — update an existing item. Use for corrections, not for new facts.
- `codeterm mem delete <id>` — remove a specific item (also cleans vectors and relations).
- `codeterm mem forget <area>` — batch delete everything in an area (e.g. "forget binaura" removes all binaura memories).
- `codeterm mem wake-up` — prints the critical L0+L1 context (identity + top-N important items). The General Agent runs this on init; regular tabs can run it too if they need a fresh overview.

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

**Body quality matters.** The body gets chunked and embedded as vectors — include names, project references, technologies, and context. A body like "fixed the bug" is useless. "Fixed JWT refresh 401 by adding kid header from JWKS endpoint, Auth0 requires it" is searchable.

## Filters

Add `--kind <kind>` to narrow by type. Valid kinds: `skill`, `fact`, `note`, `conversation`, `script`, `peer`, `preference`, `daily`, `learning`, `rule`, `summary`.

Add `--area <tag>` to narrow by subject area (e.g. `auth`, `billing`, `deploy`, `home`, `automation`).

Add `--scope workspace` to restrict to the current workspace's mem_items (global items are always included by default).

Add `--limit <n>` to change the result count.

## Graph queries (temporal facts and relationships)

- `codeterm mem graph related --id <id>` — what's connected to this thing
- `codeterm mem graph create --id <id> --entity-type <Type> --scope global` — create entity
- `codeterm mem graph relate --from <id> --rel <rel> --to <id>` — create relation
- Add `--as-of <date>` for point-in-time queries; stale / invalidated facts are filtered automatically

## Escalation order when a search misses

1. **Try literal keywords first** — `codeterm mem search "home automation"`. Fast, cheap, deterministic.
2. **If no clear winner, broaden semantically** — `codeterm mem recall "what can I use to control lights"`. Catches paraphrases and near-synonyms.
3. **Follow graph relationships** — `codeterm mem graph related --id skill_home_assistant` to find scripts, references, and related items that aren't obviously matched by the query text.

Do NOT load every skill upfront. The store is lazy — fetch specific bodies only after a search identifies what's relevant.

## Invariants you must respect

- **Persistent memory lives in mem, nowhere else.** You never save anything important to `~/.claude/projects/*/memory/` or provider-specific auto-memory — those vanish on provider switch. Every `codeterm mem save` survives restarts, provider switches (Claude ↔ Codex), and CodeTerm upgrades.
- **Never edit `CLAUDE.md`, `AGENTS.md`, or `AGENT.md`** — those are auto-generated by CodeTerm on every start. To persist context, use `codeterm mem save` instead.
- **Never write skill files into `~/.claude/skills/` or `~/.agents/skills/`** — those are user territory. If the user asks you to create a skill, save it as a mem_item: `codeterm mem save --kind skill --title <name> "<skill body>"`.
- **Every `save` should be specific.** `codeterm mem save --kind fact "API timeout was fixed by bumping the deadline to 30s"` is useful. `codeterm mem save --kind note "did some work"` is noise.
- **When a task needs a skill you haven't loaded, search for it first** — don't guess or ask the user what skills exist. The user may have installed skills you don't know about yet.

## Examples

```bash
# User asks: "send a message to my home assistant"
codeterm mem search "home assistant notify"
# → skill_home_assistant (0.92), fact_ha_notify_ttl (0.71), script_ha_api_py (0.63)
codeterm mem get skill_home_assistant
# → full skill body with instructions

# User asks: "how do I deploy this project?"
codeterm mem search "deploy" --area deploy --limit 3
# → skill_deploy_pipeline, fact_deploy_staging_url, note_deploy_checklist

# Capturing a new learning after fixing a tricky bug
codeterm mem save --kind learning \
  --tags auth,jwt,refresh \
  "JWT refresh needs the kid header even though the spec says it's optional — our issuer requires it"

# Graph query: what does this skill depend on?
codeterm mem graph related --id skill_home_assistant --rel has_script
# → script_ha_api_py
codeterm mem get script_ha_api_py
# → runnable Python wrapper

# Fresh session orientation (runs automatically on GA init)
codeterm mem wake-up
# → prints L0 identity (~100 tok) + L1 critical facts (~600 tok)
```

## If mem is empty

On a brand-new install, `codeterm mem search` returns nothing. That's expected. Capture things as you go via `codeterm mem save`, and the store grows from use. Bundled starter items (a few baseline rules, preferences, and the `codeterm` skill itself) ship with every install and are always present.

## System power (keepawake)

When the user asks to prevent sleep, keep the machine awake, or "don't sleep for N hours", use `codeterm keepawake` — never `caffeinate`, `powercfg /requests`, or `systemd-inhibit` directly. CodeTerm tracks the session cross-platform and exposes it to the UI tray.

- `codeterm keepawake start --duration-secs 7200` — keep system awake for 2 hours (add `--display` to also prevent display sleep)
- `codeterm keepawake start` — indefinite (until explicit stop)
- `codeterm keepawake stop` — release
- `codeterm keepawake status` — current session (active, display, expires_at)
