# Handoff — next Claude on another machine

You are picking up CodeTerm `feat/memory` branch mid-work. Read this **before touching anything**, then delete this file when done (it's a one-shot note, not a permanent doc).

## Where we are

Branch: `feat/memory` (pushed to origin, head = `5079364`).
Last two commits:
- `5079364` feat(dispatch): Chunk C batch 2 — PTY ops through unified dispatch
- `ebcc50b` feat(mem): Phase 3.0 — bridge rule + drop destructive skill mirror

`./build.sh check` clean, `./build.sh test` = **406 passing**, dev instance verified live (build #60, port 7686).

## What just shipped (Phase 3.0)

The old `sync_watcher` mirrored bundled skills into `~/.claude/skills/` — destructive, user territory. That's gone. Now:

- `src-tauri/prompts/bridge_mem.md` — static one-page payload teaching `codeterm mem search/get/save/recall/wake-up`. This is what every agent reads at session start.
- `AgentProvider::install_bridge_rule(payload)` trait method — each provider decides where to drop the payload. `ClaudeProvider` writes atomically to `~/.claude/rules/codeterm.md`. Codex/others = no-op default until implemented.
- `sync_watcher::install_bridge_rules_for_all` runs on startup and on `ContributionsChanged { kind: "agent_provider" }` (wired in `lib.rs` setup closure).
- `regenerate_agents_md` inlines `BRIDGE_MEM` so `AGENTS.md` stays in sync.
- `canonical_skills_dir` legacy helper deleted; callers inline `crate::paths::data_dir().join("skills")`.

**The bridge rule is live and references CLI commands that don't exist yet.** That's intentional — Phase 3.1 builds them. Until then `codeterm mem *` will fail; agents reading the rule will think mem exists. Don't panic, just push 3.1.

## What's next — Phase 3.1 (the big one)

Source of truth: **`docs/plans/mem-architecture.md`** (read it cover-to-cover, it's ~1400 lines, all the design decisions are there). TL;DR is at the top.

Goal: implement the unified `mem` core (NOT a plugin — it's core, plugins depend on it). Native Rust + SQLite. We ported ideas from MemPalace (cloned to `/references/mempalace/`, gitignored) but rejected Python/ChromaDB/AAAK dialect/palace vocab.

Concrete deliverables for Phase 3.1:
1. **`mem_items` table + `relations` table** — schema is in `mem-architecture.md`. One table, `kind` discriminator (`skill`, `fact`, `note`, `conversation`, `script`, `peer`, `preference`, `daily`, `learning`, `rule`, `summary`). Temporal validity via `valid_from`/`valid_to`.
2. **Rust store module** — `src-tauri/src/mem/` (new). CRUD + FTS5 search. Tests inline.
3. **Graph engine** — `relations` traversal, `as-of` queries.
4. **CLI subcommand** `codeterm mem` in `cli/` crate with `search/get/save/recall/wake-up/graph` arms. The bridge rule (`src-tauri/prompts/bridge_mem.md`) documents the exact UX — match it.
5. **Wake-up = L0 identity + L1 critical items.** Layered stack design is in mem-architecture.md.
6. **Bundled starter items** seeded on first install (a few baseline rules + the `codeterm` skill itself as a mem_item).

`sqlite-vec` is **opt-in** for Phase 3.5 — don't pull it in 3.1, FTS5 only. `recall` falls back to FTS5 when vectors are off.

## Parallel work you can spawn while doing 3.1

**Chunk C batches 3 and 4** are still pending (agent ops, mesh ops through dispatch). These are mechanical refactors — perfect for `backend-dev` subagent. Pattern is set by batch 1 (commit `08f2634`) and batch 2 (`5079364`). Tell the subagent to read those commits + `src-tauri/src/dispatch/mod.rs` and follow the template. Don't let it touch anything in `src-tauri/src/mem/` — that's yours.

**PtyExec is deferred** — needs async dispatch, scope decision required. Don't tackle it inside batch 3/4.

## Working tree state

Clean. Two `BUILD_NUMBER` / `Cargo.toml` modifications you'll see on first build are line-ending warnings, ignore them.

## Verification ritual before debugging anything

`./build.sh dev` increments `BUILD_NUMBER`. ALWAYS check:
```bash
curl -s http://127.0.0.1:7686/api/health | jq '{build, build_number}'
```
Match against `git rev-parse --short HEAD`. If they don't match — old build, your changes aren't live, don't waste time.

## Memory note

There's a feedback memory `feedback_code_comments.md` saying: minimal inline comments, design rationale belongs in `docs/plans/`, not `.rs` files. Respect it. The user **will** call you out if you write multi-line module headers.

## Don't

- Don't commit/push without explicit "коммить" / "push".
- Don't write skills into `~/.claude/skills/` — that's exactly what we just removed.
- Don't add legacy back-compat shims. `не надо легаси оставлять`.
- Don't `cat` agent definitions into spawn prompts — tell agents to READ them.

## Questions to ask the user before starting 3.1

1. Should `mem_items.id` be human-readable slugs (`skill_home_assistant`) or ULIDs? The bridge rule examples use slugs, so probably slugs — but confirm.
2. Workspace-scoped vs global default for `save` — bridge rule says global default, workspace via `--scope workspace`. Confirm.
3. `wake-up` token budget: L0 ~100 + L1 ~600 from the bridge rule. Hard cap or soft?

That's it. Read `mem-architecture.md`, ask the three questions above, then start with the schema migration.

---

**Delete this file after reading.** It's a one-shot handoff, not a permanent doc.
