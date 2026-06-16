# DOCS-SYNC-POST-PURGE — close the gaps the CLI purge left in docs/skills

**Executor:** fresh codex worker · **Branch:** stabilization · **Effort:** S
**Backlog ref:** follows CLI-PURGE (commit a4fdeb96)

## Goal
No projection of the CLI surface still teaches deleted commands, and docs/API.md documents the REST fields the purge added.

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK DOCS-SYNC-POST-PURGE ACCEPTED, starting" --pane 5c64a651
```

## Evidence (run these FIRST; if output differs, report BLOCKED "evidence stale")
```
git log --oneline -1 -- cli/                                      # expected: a4fdeb96 fix(cli): purge duplicate commands...
git grep -n "agent hierarchy" -- docs/SKILLS.md                   # expected: empty (repo already swept)
```

## Steps (in order, no skipping, no additions)
1. docs/API.md: document the new REST fields — `cmd` on POST /api/panes (create shell pane: runs the command in the fresh pane via the verified send path) and `changed_by` on the mem item update body (optional audit actor, sanitized, falls back to headers). Note `capabilities` as socket-only (UNSUPPORTED over mesh). Bump the revision tag. Verify against the actual code (src-tauri/src/api/pane_routes.rs, src-tauri/src/api/mem_routes/items.rs) — document what IS, not what the purge report said.
2. ON-DISK SKILL SYNC (outside the repo, no git involved): the canonical user-scoped skill folder `~/.codeterm/skills/codeterm/` (auto-mirrors to ~/.claude/skills and ~/.agents/skills) still teaches deleted commands. Sweep SKILL.md and every references/*.md in that folder for: `agent hierarchy`, `agent list`, `set-conductor`, `paste pop`, `mesh send` (the subcommand — `send --mesh` flag stays), top-level `update --check/--apply/--channel`. Replace with the surviving equivalents (`pane list --agents`, `paste get <id> --pop`, `send --mesh`, `update check/install/channel`). Bump the `<!-- revision: N -->` tag in SKILL.md. Do NOT edit ~/.claude/skills directly — only the canonical ~/.codeterm/skills path.
3. Sanity: `git grep -n "agent hierarchy\|paste pop\|set-conductor" -- docs/ src-tauri/prompts/ .codeterm/` must be empty; same grep over the skill folder must be empty.
4. List every file you touched in the skill folder explicitly in the report (they're outside git — the report is the only record).

## Verify (run all; paste real output in the report)
```
git grep -n "agent hierarchy\|paste pop\|set-conductor" -- docs/ src-tauri/prompts/ .codeterm/   # must: empty
codeterm mem search "codeterm skill"                              # must: returns the skill (Librarian reindex picks up the edit; note result)
```

## Forbidden (beyond the global rules in README.md)
- NO git mutations — report repo paths + proposed commit message; I commit.
- Do not touch cli/, src/, evals/, tests/.
- Do not rewrite historical docs (docs/review-*, docs/release/, docs/plans/v*, docs/superpowers/) — live docs only.

## Report
```
codeterm send "TASK DOCS-SYNC-POST-PURGE STATUS ... CHANGED(repo): ... CHANGED(skill folder): ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
