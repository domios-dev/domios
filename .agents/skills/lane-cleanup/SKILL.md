---
name: lane-cleanup
description: Safely audit and remove completed or abandoned Domios lane worktrees and Windows filesystem residue. Use when git worktree removal fails, a lane directory remains after cleanup, Windows reports access denied, reserved names such as NUL may exist, worktree pressure is high, or a completed task needs tab/worktree/branch cleanup without losing dirty or unmerged work.
---

# Lane Cleanup

For Plan-wide cleanup, preview with `domios plan lanes <plan> --clean`, then
execute with `--confirm`. The live Plan Orchestrator may run this from a Team
utility shell it created. Keep the shell's own tab id and launch nonce; never
copy the parent's session or binding. The daemon checks recorded ownership and
the live Plan owner. Dirty, unmerged and occupied lanes remain protected.

The preview also inventories unregistered residue under the lane root. Confirm
removes only artifact directories without `.git` or tracked source, and checks
registration and live holders again immediately before deletion. Unknown content
stays protected. Removal unlinks reparse points without following their targets,
uses extended Windows paths, verifies absence and reports exact failed paths
with Windows holder evidence when available.

Acceptance defers cleanup while the Worker is live and retries when it closes.
Clean merged lanes lose their worktree and branch (`git branch -d`). Cancellation
may remove a clean unmerged worktree while retaining its branch for recovery.
Dirty lanes stay intact; reminders retain cleanup debt and its exact command.
`build.sh gc` trims expired lane `target`, `src-tauri/target` and `cli/target`
outputs using the configured build-cache retention, protecting recent artifacts,
tracked content and the calling checkout.

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
4. Any owning Domios tab supplied by the caller is closed first.
5. Windows reserved-name artifacts are removed through exact extended paths.

Before recursive deletion, the script walks real directories explicitly and
unlinks every reparse point without entering its target. Never recursively
delete through a lane's `node_modules` junction: it may target canonical.

Never use this skill on the canonical checkout, a dirty lane, an unmerged lane,
or an unknown directory. Do not kill processes by name. If deletion still fails,
report the exact path and holder evidence; preserve the branch and task cleanup
state until the handle is released.

For a directory already unregistered by Git, require the caller to re-check the
task's recorded ancestry/cleanup evidence and pass `-AllowUnregisteredResidue`.

After successful deletion, remove the merged branch with `git branch -d` and
record task cleanup as done. Use `-PlanId` and `-TaskId` to let the script record
that state automatically.

When you do not already know which tab owns the lane, prefer the daemon-side verb
`domios plan lane-remove <worktree> --branch <branch>`: it names the live tab
holding the directory and refuses before anything is deleted.
