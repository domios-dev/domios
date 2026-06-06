# Memory (mem) — full reference

The always-on protocol that fires regardless of which skill is loaded lives in `~/.claude/rules/codeterm.md`. This reference is the detailed how-to: commands, kinds, filters, graph queries, search strategy, skill-folder workflow.

## Commands

| Command | Purpose |
|---------|---------|
| `codeterm mem search "<query>" [--kind K] [--area A] [--limit N] [--rerank]` | Hybrid search (FTS5 + vector embedding fused via RRF). Default limit 5. |
| `codeterm mem get <id>` | Print the full body of one item. Bumps `evidence_count` for `skill\|lesson\|rule\|fact\|preference` (skipped automatically when called from a worker pane via `$CODETERM_WORKER_ID`). |
| `codeterm mem save --kind <kind> [--title T] [--area A] [--tags T] [--source-path P] [--scope global\|workspace:<id>] [--workspace WS] [--pin] [--importance N] [--replace] [--skip-extract] "<body>"` | Capture something new. `--pin` sets `is_pinned_wakeup=1` in the same INSERT (atomic, no follow-up PATCH). `--workspace WS` is shorthand for `--scope workspace:WS`. |
| `codeterm mem update <id> [--title T] [--summary S] [--area A] [--tags T] [--body B] [--importance N] [--changed-by ACTOR]` | Patch an existing item. `--changed-by` sets the `X-CodeTerm-Actor` header so the change lands in `mem_item_history.changed_by` with that label (default `"user"`). |
| `codeterm mem delete <id>` | Remove an item (also cleans vectors and relations). |
| `codeterm mem forget <area>` | Batch delete every item in an area. |
| `codeterm mem history <id>` | Full audit trail (create/update/promote/delete) for one item. |
| `codeterm mem taxonomy [--workspace W]` | Print `{area → {kind → count}}` for the store or one workspace. |
| `codeterm mem tail [--follow] [--limit N]` | Recent mem writes, newest first. |
| `codeterm mem stats` | Counts by kind + blob sizes. |
| `codeterm mem wake-up [--budget N] [--l0-only] [--no-l0] [--workspace WS] [--explain]` | L0 identity + L1 critical facts. `--workspace` boosts items scoped to that workspace by `+0.5` in the L1 ranking and admits them alongside global rows. `--explain` appends a per-item score breakdown comment block. Always use this for identity questions. |
| `codeterm mem pin <id>` | Set `is_pinned_wakeup=1` so the item always loads in L1 wake-up (subject to budget). |
| `codeterm mem unpin <id>` | Clear `is_pinned_wakeup`. |
| `codeterm mem pinned [--limit N]` | List pinned items, ordered by importance / evidence / recency. Default 100, max 500. |
| `codeterm mem learn "<text>" [--kind insight\|error\|feature] [--tags a,b]` | Quick capture of an insight. `--error <text>` / `--feature <text>` are shortcuts that set the kind. Stored as `kind=learning`. |
| `codeterm mem learn-list` | List pending learnings (the `kind=learning` items). |
| `codeterm mem learn-promote <id>` | Promote a learning to a fact (raises its importance). |
| `codeterm mem sync [--path P]` | Trigger Librarian re-index for source-tracked items. |
| `codeterm mem extract-pending [--limit N]` | Re-queue extraction workers (after API quota or interrupted runs). |
| `codeterm mem calibration log [--since ISO] [--limit N] [--count]` | Audit trail of the dreamer's Sunday calibration sweep (rows in `mem_item_history` with `changed_by LIKE 'dreamer-calibration%'`). |
| `codeterm mem calibration sweep [--force]` | Manually run the sweep. Server-gated to Sunday unless `--force`; hard-capped at 30 mutations per session. |

## Saving — what to use as what

| Kind | Use for | Example body |
|------|---------|--------------|
| `fact` | Verified decisions, configs, architecture choices | "PgBouncer with transaction mode, max_client_conn=400" |
| `learning` | Bug fixes, gotchas, things you figured out | "JWT kid header required by Auth0 even though RFC says optional" |
| `preference` | User preferences, style choices | "Always use vim keybindings, prefer Dracula theme" |
| `note` | Meeting notes, conversations, context | "Talked to Tima about audio engine architecture" |
| `skill` | How-to instructions, reusable procedures | "How to deploy to fly.io: run ./build.sh build then..." |
| `rule` | Hard constraints — never/always | "Never commit .env files" |
| `peer` | Machine / service info | "Mac Mini runs Docker with HA, Mosquitto, a gateway" |
| `document` | Indexed files tracked on disk | Saved via `--source-path`, auto-synced by Librarian |

Body quality matters. The body is chunked and embedded as vectors — include names, project references, technologies, and context. "fixed the bug" is useless. Save like:

```bash
codeterm mem save --kind learning --area auth \
  "Fixed JWT refresh 401 by adding kid header from JWKS endpoint, Auth0 requires it"
```

For documents on disk:

```bash
cat /path/to/doc.md | codeterm mem save --kind document \
  --source-path "/path/to/doc.md" \
  --title "descriptive title for search"
```

The Librarian re-indexes the source whenever the file changes.

## Filters

| Flag | Effect |
|------|--------|
| `--kind <kind>` | Narrow to one kind. Valid: `skill`, `fact`, `note`, `conversation`, `script`, `peer`, `preference`, `daily`, `learning`, `rule`, `summary`, `document` |
| `--area <tag>` | Narrow to a subject area (e.g. `auth`, `billing`, `deploy`, `home`, `automation`) |
| `--scope workspace` | Restrict to current workspace's items (global is always included) |
| `--limit <n>` | Result count |

## L1 wake-up — pinning, workspace, score

`codeterm mem wake-up` returns L0 (identity) + L1 (top critical context). L1 selection is two-pass:

1. **Pinned set** — `is_pinned_wakeup = 1` in scope (global + current workspace if `--workspace` is set). Pinned items always load and render before any ranked item inside their kind section, prefixed with `★`.
2. **Ranked fill** — remaining items scored by `importance + log10(1 + evidence_count) * 0.5 + ws_match * 0.5 + recent_30d * 0.3`. `auto-extracted` noise and the L0 identity row are excluded.

Budget defaults to `[memory] l1_budget_tokens` (default 300). When the pinned set alone exceeds the budget the rendered output starts with `<!-- WARN: N pinned items did not fit L1 budget … -->` and a `[wake-up] pinned-eviction` log line fires — shrink pins (`mem unpin <id>`) or raise the budget.

```bash
# Pin a load-bearing fact for the current workspace
codeterm mem save --kind fact --title "API base url" --pin --workspace "$CODETERM_WORKSPACE_ID" \
  "Staging API base: https://api-staging.example.com (no trailing slash)"

# Workspace-aware wake-up — boosts ws-scoped rows in the score
codeterm mem wake-up --workspace "$CODETERM_WORKSPACE_ID"

# Debug what L1 selected
codeterm mem wake-up --workspace "$CODETERM_WORKSPACE_ID" --explain | tail -30

# Inspect / curate pinned set
codeterm mem pinned --limit 50
codeterm mem unpin fact_old_thing
```

## Calibration — Sunday sweep

The dreamer's Sunday run includes a calibration STAGE D that reviews items 14+ days old and applies bounded importance / archive changes:

| Condition | Action | `changed_by` label |
|---|---|---|
| `evidence_count >= 10 AND importance < 4 AND accessed in last 30d` | `+0.5` importance | `dreamer-calibration:promote` |
| `evidence_count <= 1 AND importance >= 4 AND created > 60d ago` | `-0.5` importance | `dreamer-calibration:demote` |
| `evidence_count == 1 AND importance <= 2 AND created > 90d ago AND kind IN (fact, note)` | archive (`kind = 'archived'`) | `dreamer-calibration:archive` |

Hard caps: max 30 mutations per session, abort if candidate set > 100, refuse if any calibration change landed in the last 6h. Day-of-week gate is server-side (`strftime('%w','now') == '0'`); `--force` bypasses for testing.

```bash
codeterm mem calibration log                  # last 50 changes (default)
codeterm mem calibration log --count          # just the row count
codeterm mem calibration log --since "2026-05-01T00:00:00Z" --limit 100
codeterm mem calibration sweep --force        # manual run (testing only)
```

## Search strategy when results miss

1. Short keyword first — `codeterm mem search "home automation"`. Best when you know the term.
2. Reformulate as a natural phrase — `codeterm mem search "what controls the lights at home"`. Embedding catches semantic intent.
3. Follow graph relationships — `codeterm mem graph related --id skill_home_assistant`.

Don't give up after one query. If the first call returns nothing useful, restate with different terms.

## Graph (temporal facts and relationships)

```bash
codeterm mem graph related --id <id>                                   # outgoing relations from this entity
codeterm mem graph create --id <id> --entity-type <Type> --scope global # create entity
codeterm mem graph relate --from <id> --rel <rel> --to <id>            # create relation
codeterm mem graph timeline --entity <id>                              # full temporal history
codeterm mem graph traverse --from <id> --hops 2                       # BFS
codeterm mem graph path --from <id> --to <id>                          # shortest path
codeterm mem graph tunnels                                             # entities bridging 2+ areas
```

Add `--as-of <date>` for point-in-time queries; stale / invalidated facts are filtered automatically.

Graph entities are auto-created by the extraction worker. Entity types: `Person`, `Project`, `Tech`, `Machine`. Relations are free-form verb predicates (`works_on`, `uses`, `shipped`, `runs_on`, etc).

## Skill folder workflow

A skill is a **folder**, not a single file. `SKILL.md` is the index — it gets vectorized into mem. The folder may also contain executable scripts, example payloads, snippets — anything the skill needs.

```
<skills-dir>/deploy-to-staging/
  SKILL.md           ← what gets indexed
  scripts/deploy.sh  ← real bash, callable from SKILL.md steps
  examples/payload.json
```

Always create new skills via `codeterm mem skill new <slug>` — it picks the right `<skills-dir>` for the current instance (dev = `~/.codeterm-dev/skills/`, prod = `~/.codeterm/skills/`), scaffolds SKILL.md with frontmatter + sections, and registers in mem immediately.

```bash
codeterm mem skill new deploy-to-staging --area infra
# CLI prints the folder path. Flesh out SKILL.md and add scripts there.
```

`codeterm mem skill add <folder>` is for **importing** an existing folder (cloned repo, copied from another machine).

When a search surfaces a skill, the body holds `SKILL.md` content. To see the full folder and execute scripts: `ls $(dirname <source_path>)`. Editing any file → Librarian's hourly scan re-indexes; `codeterm mem sync` triggers immediately.

## Sessions (past chats)

```bash
codeterm sessions search "<query>" [--provider P] [--project P] [--limit N]
codeterm sessions get <id>                                             # provider, cwd, file path
codeterm sessions resume <id>                                          # atomic resume — creates workspace + agent + --resume
```

Always prefer `sessions resume` over manual `workspace create → agent spawn --args=--resume=UUID` — the manual path keeps breaking because `claude --resume` needs the JSONL in the current encoded project dir.

**Rule of thumb:** past conversation → `sessions search`. Durable fact → `mem search`.

## Keepawake (system power)

When the user wants to keep the system from sleeping, use `codeterm keepawake` — never `caffeinate`, `powercfg`, or `systemd-inhibit` directly.

```bash
codeterm keepawake start --duration-secs 7200       # 2 hours
codeterm keepawake start --duration-secs 7200 --display  # also prevent display sleep
codeterm keepawake start                            # indefinite (until explicit stop)
codeterm keepawake stop
codeterm keepawake status
```

<!-- revision: 3 -->
