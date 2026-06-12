# D9 — PR template + review checklist + issue form

**Executor:** any claude pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** D9 in ../stabilization-backlog.md
**DO NOT START before D6 is reported done** (both touch docs/TESTING.md).

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send-text "TASK D9 ACCEPTED, starting" --pane 5c64a651
```

## Goal
PRs carry the evidence rules (fail-first proof, verification output); issues carry
the scope-prefix convention. Both enforced by templates, not memory.

## Evidence (run FIRST)
```bash
ls .github/                                     # expected: workflows/ only (+ nothing template-ish)
grep -n "scope" CLAUDE.md | grep -i issue       # the <scope>: title convention to encode
```

## Steps
1. Create `.github/PULL_REQUEST_TEMPLATE.md`, UNDER 20 lines, sections:
   - `## What` (1-2 sentences)
   - `## Verification` — "paste real command output; claims without output don't count"
   - `## New/changed tests` — "fail-first method used (bug-fix red→green / sabotage / refactor 3×green) + evidence; layer justification (see docs/TESTING.md decision tree)"
   - `## Docs` — "revision tags bumped on every touched doc? file-size check green?"
2. Create `.github/ISSUE_TEMPLATE/config.yml` (`blank_issues_enabled: true`) and one
   form `.github/ISSUE_TEMPLATE/task.yml`: title placeholder `<scope>: description`,
   a dropdown with the scopes from CLAUDE.md (fix, feat, mem, mesh, research, docs,
   build), body textarea. Minimal — no labels machinery.
3. Create `docs/contributing/review-checklist.md` (reviewer side, one screen max):
   sleeps justified? fixtures used, not raw setup? tags present on new tests?
   cleanup paths? doc revisions bumped? file sizes? evidence pasted, not asserted?
4. Add one link line to `docs/TESTING.md` pointing at the checklist (bump revision).
5. Commit: `build: PR template, issue form, review checklist (evidence discipline)`

## Verify
```bash
ls .github/PULL_REQUEST_TEMPLATE.md .github/ISSUE_TEMPLATE/task.yml docs/contributing/review-checklist.md   # all exist
wc -l .github/PULL_REQUEST_TEMPLATE.md          # must: <= 20
grep -n "review-checklist" docs/TESTING.md      # must: 1 hit
```

## Forbidden
- No CODEOWNERS, no labels automation, no extra workflows — exactly the three files
  plus the one TESTING.md line.

## Report
```
codeterm send-text "TASK D9 STATUS ... CHANGED: ... VERIFIED: ... NOTES: ..." --pane 5c64a651
```
