# B10 — Fix the shipped codeterm skill: it contradicts the running binary

**Executor:** Sonnet pane · **Branch:** stabilization · **Effort:** M
**Backlog ref:** B10 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK B10 ACCEPTED, starting" --pane 5c64a651
```

## Goal
The `codeterm` skill (the manual every in-app agent loads) matches the binary:
2-level role model, only real subcommands, no references to emptied
`.claude/agents/`. Fixed at its SOURCE in the repo, not just the synced copy.

## Evidence (run FIRST)
```bash
grep -n "Lead" ~/.claude/skills/codeterm/SKILL.md | head        # expected: 3-level hierarchy claims
grep -n "agent status" ~/.claude/skills/codeterm/SKILL.md       # expected: documented
codeterm agent status 2>&1 | head -2                            # expected: unrecognized subcommand
grep -n ".claude/agents" ~/.claude/skills/codeterm/SKILL.md     # expected: worker template reference
grep -rn "exactly 3 roles\|Lead" skills/ --include="*.md" -l 2>/dev/null   # find the in-repo source
```

## Steps
1. Find the skill's source of truth in the repo: check root `skills/` dir first;
   if absent, grep the repo for a distinctive phrase from the skill
   (`"spinal cord"` or `"Auto-name your tab"`). Identify how it reaches
   `~/.claude/skills/codeterm/` (watcher? install step? release artifact?). Report
   the sync mechanism — this is half the task's value.
2. In the SOURCE file, fix:
   a. Hierarchy section → 2 roles (Orchestrator → Worker), matching
      `codeterm agent spawn --help`. Remove the Lead row/diagram/decision-table
      entries; remove "Lead under Lead" rejection notes.
   b. Remove `codeterm agent status` from the CLI listing (or, if an equivalent
      exists under another name, document that instead — check
      `codeterm agent --help`).
   c. Worker task template: replace `Read .claude/agents/backend-dev.md` with
      `codeterm mem get <role_skill_id>` phrasing (see CLAUDE.md's role table).
3. Sweep the rest: for EVERY `codeterm <sub> <cmd>` the skill documents, run
   `codeterm <sub> --help` and confirm the command exists with the documented
   flags. Build a mismatch table; fix the skill text for each mismatch.
4. Bump the skill's `<!-- revision: N -->`.
5. Verify the synced copy at `~/.claude/skills/codeterm/SKILL.md` updates (run the
   sync mechanism found in step 1, or document the manual step in the report).
6. Commit: `docs(skill): sync codeterm skill with binary (2-level roles, real commands)`

## Verify
```bash
grep -c "Lead" <source-file>                    # must: 0 role-related hits (paste residual lines if any are plain English)
grep -n "agent status" <source-file>            # must: 0
diff <source-file> ~/.claude/skills/codeterm/SKILL.md   # must: empty after sync
```

## Forbidden
- Do not edit `~/.claude/skills/codeterm/SKILL.md` directly as the fix — that copy
  is overwritten by sync. Fix the source; sync propagates.
- Do not redesign the skill's structure; content corrections only.

## Report
```
codeterm send-text "TASK B10 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <sync mechanism; mismatch table>" --pane 5c64a651
```
