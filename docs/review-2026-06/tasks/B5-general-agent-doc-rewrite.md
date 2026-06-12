# B5 — GENERAL_AGENT.md: rewrite to implemented reality, move fiction to plans

**Executor:** Gpt5.5 pane · **Branch:** stabilization · **Effort:** M
**Backlog ref:** B5 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK B5 ACCEPTED, starting" --pane 5c64a651
```

## Goal
Every claim in docs/GENERAL_AGENT.md maps to a file:line in the code. Unbuilt
design material moves to docs/plans/general-agent-future.md (indexed in
docs/PLANS.md), clearly labeled as future design.

## Evidence (run FIRST)
```bash
grep -n "CODETERM-AUTO-START" docs/GENERAL_AGENT.md | head -3      # doc claims markers
grep -rn "test_agent_template_no_markers" src-tauri/src/service/general_agent/   # code asserts NO markers
grep -rn "EXTRA_INSTRUCTIONS" src-tauri/src/ | head -2             # must: zero hits
grep -n "connected_services" src-tauri/prompts/general_agent.md    # must: zero hits (placeholder is fiction)
```

## Steps
1. Read the implementation: `src-tauri/src/service/general_agent/` (all files) and
   `src-tauri/prompts/general_agent.md`. Build your own list of what ACTUALLY
   exists: template placeholders + their substitution (instructions.rs), file
   layout on disk, lifecycle (when regenerated), wake-up integration.
2. Sort every section of the current GENERAL_AGENT.md into KEEP (matches code),
   FIX (drifted detail), or FICTION (markers, EXTRA_INSTRUCTIONS.md, six phases,
   Discord/webhook/autostart, {connected_services}).
3. Rewrite docs/GENERAL_AGENT.md from the KEEP+FIX material — structure:
   what GA is / file layout / template + placeholders table (each with the
   instructions.rs line that fills it) / lifecycle / how to extend. Bump revision.
4. Move FICTION content into docs/plans/general-agent-future.md with a header
   "Design backlog — NOT implemented. Extracted from GENERAL_AGENT.md 2026-06-11."
   Add an index line to docs/PLANS.md (bump its revision).
5. Report changed paths + proposed commit msg:
   `docs: GENERAL_AGENT.md describes implemented reality; future design moved to plans`

## Verify
```bash
grep -c "CODETERM-AUTO-START\|EXTRA_INSTRUCTIONS\|connected_services" docs/GENERAL_AGENT.md  # 0
grep -n "general-agent-future" docs/PLANS.md            # 1 hit
# spot-check: pick 3 placeholder rows from your new table, paste the instructions.rs lines that substantiate them
```

## Forbidden
- No code changes; docs only.
- Do not delete the fiction — it moves to plans (the ideas are unbuilt, not wrong).
- Git arbiter-only.

## Report
```
codeterm send-text "TASK B5 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <KEEP/FIX/FICTION counts>" --pane 5c64a651
```
