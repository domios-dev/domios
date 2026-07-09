---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0cd0f8c970ca42e492558c26aefee1c9
handle: p0-report-reliability-ms_0cd0f8c970ca42e492558c26aefee1c9
title: report_refused
---

PARTIAL: REPORT NOT SENT
readiness report refused: this git worktree has uncommitted lane changes.
Next commands:
  git status --short
  git add <files>
  git commit -m "<message>"
  codeterm agent report --status in_review --sha $(git rev-parse HEAD) "..."
Or report WIP with:
  codeterm agent report --status partial "..."
Dirty lane files:
  smoke-real-lane-dirt-55876.tmp

Original readiness report that was refused locally:
smoke readiness report should be refused but visible to parent
