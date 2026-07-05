---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_745d2e92855743b78059b3b4f9028857
handle: workspace-isolation
title: Workspace Isolation
---

**HOW workspaces are isolated.** One workspace = one project root (the git toplevel of its cwd) = one `.codeterm/plans` dir. The canvas in a workspace shows only plans whose `project_path` canonicalizes to that root, which fixes a real leak where one project's plans bled onto another's board. The resolution is worktree-aware in `paths.rs` — a linked worktree that committed its own plans on its branch owns its store, otherwise it falls back to the main worktree:

```rust
pub fn project_root(cwd: &Path) -> PathBuf {
    if let Some(top) = worktree_top(cwd) {
        if top.join(".codeterm").is_dir() { return top; }   // worktree owns its store
    }
    if let Some(root) = main_worktree_root(cwd) { return root; }
    if let Some(top) = worktree_top(cwd) { return top; }
    cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf())
}
```

On the frontend, `planWorkspaceScope.ts`/`workspacePlanEntry.ts` filter the projected plans to the workspace's root before layout, so the board is scoped by construction — a plan from a sibling project is never even handed to `layoutWorkspace`. Verified: the dogfood plan appears in the codeterm project and is absent from a sibling project's canvas.

## Key decisions
- **Canonicalized project root as the scope key** — symlinks/relative cwds resolve to one identity, so "same project" is unambiguous.
- **Worktree owns its store when it has `.codeterm`** — feature-branch worktrees keep their committed plans visible instead of resolving to main (a different branch's plans); without this, branch-local plans vanish.
- **Filter before layout, not after render** — scope is enforced at the data boundary, so no off-workspace node is ever positioned or painted.
