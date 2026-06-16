# B4 — CLAUDE.md factual sync: eval sets + .codeterm/skills list

**Executor:** Haiku or Sonnet pane · **Branch:** stabilization · **Effort:** S
**Backlog ref:** B4 in ../stabilization-backlog.md

## Step 0 — confirm receipt (ALWAYS, before anything)
```
codeterm send "TASK B4 ACCEPTED, starting" --pane 5c64a651
```

## Goal
Every path CLAUDE.md names exists on disk; the eval-set list matches
`evals/README.md` (the source of truth); the `.codeterm/skills/` list matches the
directory.

## Evidence (run FIRST)
```bash
ls evals/                                  # 8 dirs + README; note: NO general-agent/
grep -n "general-agent\|mem-protocol" CLAUDE.md | head
ls .codeterm/skills/                       # expected: ONLY github-release/
grep -n "prod-build\|self-install-local" CLAUDE.md | head
ls evals/mem-protocol/                     # expected: only last-report.md (husk)
head -30 evals/README.md                   # the real eval-set table
```

## Steps
1. CLAUDE.md "Behaviour benches" section: replace the 4-item list with the real
   sets per `evals/README.md` (cli-agent — note it REPLACED general-agent and
   mem-protocol; mem-recall; dreaming; session-restore). Add one line: "also
   present, experimental/historical: dreamer-repair, seeds, v1_1_23_e2e" — first
   check each of those three for a README; if a README declares a status, reflect
   it instead of guessing.
2. Delete the `evals/mem-protocol/` husk dir (`git rm -r`) — superseded per
   evals/README. Keep `last-report.md` by moving it to
   `docs/review-archive/mem-protocol-last-report.md`.
3. CLAUDE.md `.codeterm/skills/` section: trim the list to `github-release/` only,
   and add: "prod build lives in `build.sh build`; Windows self-install in
   `scripts/self-install-local.ps1`" (verify that ps1 path exists first; if it
   doesn't, grep scripts/ for the real name).
4. Sync `.claude/skills/prompt-evaluator/SKILL.md`: its references to eval sets
   must match the same reality (it may point to evals/general-agent — fix to
   cli-agent or to whatever its runner.py actually expects: READ runner.py's eval
   path handling before editing, and report if runner.py itself is broken).
5. Commit: `docs: sync CLAUDE.md eval sets + .codeterm/skills with reality`

## Verify
```bash
# every path mentioned in the changed sections exists:
grep -oE "evals/[a-z0-9_-]+" CLAUDE.md | sort -u | while read p; do ls -d "$p" >/dev/null 2>&1 || echo "MISSING: $p"; done   # must: no MISSING lines
ls evals/mem-protocol 2>&1                  # must: No such file
```

## Forbidden
- Do not rewrite other CLAUDE.md sections (it's the project's primary instruction
  file — surgical edits only).
- Do not touch evals/ contents beyond the mem-protocol husk removal.

## Report
```
codeterm send "TASK B4 STATUS ... CHANGED: ... VERIFIED: ... NOTES: <experimental-set statuses found; runner.py state>" --pane 5c64a651
```
