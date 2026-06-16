# B1 — Purge dead Lead-role wording from live docs and skills

**Executor:** Haiku pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** B1 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK B1 ACCEPTED, starting" --pane 5c64a651
```

## Goal
No living doc or skill describes a "Lead" agent role or `lead_suffix.md`. The
hierarchy is 2-level: Orchestrator → Worker. Historical analysis docs stay untouched.

## Evidence (run FIRST)
```bash
grep -in "lead" docs/CLI.md | head            # expected: hits at ~line 33 ('orchestrator or lead') and ~81 ('spawn leads/workers')
grep -n "lead_suffix\|\.claude/agents" .claude/skills/prompt-evaluator/SKILL.md   # expected: hits at lines ~3 and ~17
grep -n "Lead" docs/BUILD.md | head           # expected: 'Lead:' machine-role labels in release steps
```

## Steps
1. `docs/CLI.md`: fix the two role mentions — `--parent` help text becomes
   "Parent pane ID (an Orchestrator)"; set-orchestrator description becomes
   "(top-level, can spawn workers)". Bump revision tag.
2. `docs/BUILD.md`: rename the "Lead:"/"support" machine labels in the release
   procedure to "Primary:"/"Support:" (it's about machines, not agent roles —
   keep the meaning, change only the collision-prone word). Bump revision tag.
3. `.claude/skills/prompt-evaluator/SKILL.md`: remove `lead_suffix.md` from the
   description (line ~3) and from the body (line ~17); replace the
   `.claude/agents/` mention with "agent role definitions in codeterm mem".
4. Full sweep to catch stragglers:
   `grep -rin "lead" docs/ .claude/ src-tauri/prompts/ --include="*.md" | grep -vi "leads to\|leading\|leader\|misleading\|review-2026\|gap-analysis\|HANDOFF\|review-archive"`
   Fix any agent-role usage the brief missed; list each in the report.
5. Commit: `docs: purge dead Lead-role wording (2-level hierarchy: Orchestrator->Worker)`

## Verify
```bash
# the sweep grep from step 4 → only historical-doc or plain-English hits remain (paste output)
grep -c "lead_suffix" .claude/skills/prompt-evaluator/SKILL.md    # must: 0
```

## Forbidden
- Do NOT touch `docs/gap-analysis-*`, `docs/review-2026-04/`, `docs/review-2026-05/`,
  `docs/review-archive/`, `docs/review-2026-06/` — history describes the past.
- Do not edit code or prompts beyond the three files + step-4 stragglers in docs/skills.

## Report
```
codeterm send "TASK B1 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <straggler list>" --pane 5c64a651
```
