---
name: lane-cleanup
description: Safely audit and remove completed or abandoned CodeTerm lane worktrees and Windows filesystem residue. Use when git worktree removal fails, a lane directory remains after cleanup, Windows reports access denied, reserved names such as NUL may exist, worktree pressure is high, or a completed task needs tab/worktree/branch cleanup without losing dirty or unmerged work.
---

# Lane Cleanup

Use the bundled PowerShell script from the canonical checkout:

```powershell
& .\.agents\skills\lane-cleanup\scripts\cleanup-lane.ps1 `
  -LanePath 'D:\Developer\codeterm-worktrees\pl-example-task-example' `
  -Branch 'lane/pl-example-task-example' `
  -TabId 'deadbeef'
```

The script must prove all of these before deletion:

1. The resolved lane is inside the configured worktree root.
2. The lane is clean.
3. Its HEAD is an ancestor of canonical HEAD.
4. Any owning CodeTerm tab supplied by the caller is closed first.
5. Windows reserved-name artifacts are removed through exact extended paths.

Never use this skill on the canonical checkout, a dirty lane, an unmerged lane,
or an unknown directory. Do not kill processes by name. If deletion still fails,
report the exact path and holder evidence; preserve the branch and task cleanup
state until the handle is released.

For a directory already unregistered by Git, require the caller to re-check the
task's recorded ancestry/cleanup evidence and pass `-AllowUnregisteredResidue`.

After successful deletion, remove the merged branch with `git branch -d` and
record task cleanup as done. Use `-PlanId` and `-TaskId` to let the script record
that state automatically.
