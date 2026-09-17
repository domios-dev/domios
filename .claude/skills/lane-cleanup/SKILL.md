---
name: lane-cleanup
description: Safely audit and remove completed or abandoned CodeTerm lane worktrees and Windows filesystem residue. Use when git worktree removal fails, a lane directory remains after cleanup, Windows reports access denied, reserved names such as NUL may exist, worktree pressure is high, or a completed task needs tab/worktree/branch cleanup without losing dirty or unmerged work.
---

# lane-cleanup

This is a pointer, not a copy. The skill itself lives at `.agents/skills/lane-cleanup/SKILL.md`; read that file and follow it.

Everything the skill needs — scripts, helpers, references — lives beside it under `.agents/skills/lane-cleanup/`. Never edit this pointer to carry procedure, and never copy the skill here: one source, no drift.
