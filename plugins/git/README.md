# Git

Surfaces the Git state of whatever repo a pane is sitting in, without leaving the terminal.

## What it does

- **Status bubble** — a pill in the pane footer showing the current branch and a dirty/clean marker.
- **Glance popover** — click the bubble for ahead/behind, staged/unstaged counts, and recent commits.
- **Git view** — a full panel for the repo (status, log) rendered in the plugin's sandboxed UI.

All of it reads the pane's working directory; switch panes and the view follows.

## Requirements

- The `git` binary on your `PATH`. That's the only external dependency.

## Permissions

| Permission | Why |
|------------|-----|
| `subprocess: git` | Runs read-only `git` commands (`status`, `branch`, `log`) against the pane's repo. Nothing else is allowed to execute. |

No network access and no secret access — this plugin only inspects local repositories.

## Use

Install it, then open any pane inside a Git repository. The branch pill appears in the footer automatically; click it for detail.
