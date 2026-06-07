# Git Viewer: Branch Bubble + Commit Graph Panel

## Problem

CodeTerm has no way to see a repo's state visually. Users drop to `git log`
in a terminal. We want a glanceable branch indicator next to the per-pane
token-usage bubble, expanding into a full commit-graph viewer with diffs —
GitKraken-style rails, selectable/copyable diff text.

Decisions below were made autonomously (user away); rationale documented
inline. Research inputs: codebase exploration + OSS survey (vscode-git-graph,
SourceGit, pvigier's lane algorithm, React diff-lib comparison).

## Goals

- Per-pane bubble left of `UsageBubble`: short branch name + dirty dot.
  Click → full-screen git panel for that pane's repo.
- Panel: commit graph with colored branch lanes and merge curves, ref
  chips (branches/tags/HEAD), commit metadata.
- Click commit → file list with add/del stats; click file → GitHub-style
  diff (unified/split toggle), syntax highlighted, text selectable.
- Works in Tauri desktop and the web mirror (IS_WEB pattern).
- Read-only v1: no staging, committing, or branch operations.

## Non-Goals (v1)

- Any mutating git operation.
- Combined merge diffs (merges diff against first parent).
- Whole-history virtualization — v1 pages by "Load more" (300 commits/page).
- Multi-repo aggregate views; the panel shows one repo (the pane's cwd).

## Architecture

### Backend — `src-tauri/src/api/git_routes.rs` (CLI parsing, no git2)

Rationale: `git` CLI is guaranteed present for a dev tool; NUL-delimited
format parsing is trivial; avoids libgit2 surface. The existing
`agent/git_info.rs` (git2) stays as-is for the titlebar.

All endpoints take `?cwd=` (the pane's directory), run `git -C <cwd>`,
require API auth, and return 404 when cwd isn't inside a repo.

1. `GET /api/git/graph?cwd&limit=300&skip=0`
   - `git log --branches --tags --remotes HEAD --topo-order --decorate=full
     -c core.quotePath=false --pretty=format:'%x1f%H%x1f%P%x1f%an%x1f%aI%x1f%D%x1f%s%x1e'
     -n <limit> --skip=<skip>`
   - → `{ commits: [{sha, parents[], author, date, refs[], subject}],
       head_sha, branch, dirty }`
   - `refs` classified: `{name, kind: "head"|"branch"|"remote"|"tag"}`.
   - `dirty` from `git status --porcelain` (capped output).
2. `GET /api/git/commit/{sha}/files?cwd`
   - `git show -M -C --numstat --format= <sha>` (root commits handled)
   - → `[{path, old_path?, added, deleted, status}]`
3. `GET /api/git/commit/{sha}/diff?cwd&path=`
   - non-merge: `git show -M -C --no-color --format= <sha> -- <path>`
   - merge: `git diff -M -C --no-color <sha>^1 <sha> -- <path>`
   - → raw unified patch text (plain text response, capped at 1 MB with a
     truncation marker).
4. `GET /api/git/workdir-diff?cwd&path=` *(stretch, same machinery)*
   - uncommitted changes vs HEAD; lets the bubble's dirty dot lead somewhere.

Safety: `cwd` must be an existing directory; commands run with a 10s timeout
and `--no-pager`; output size capped. No shell interpolation — args passed
as a vector (std::process::Command).

### Frontend — `src/components/GitPanel/`

```
GitBubble.tsx     branch pill next to UsageBubble (reuses useGitInfo data)
GitPanel.tsx      full-screen panel (MemoryPanel pattern: toolbar + content)
gitApi.ts         IS_WEB-aware fetch wrappers (memoryApi.ts pattern)
laneGraph.ts      pure lane-assignment (assignLanes) — unit-testable
GraphView.tsx     commit rows + SVG rails column
CommitDetail.tsx  file list + diff area
styles.css        user-select: text for diffs; lane color CSS vars
```

- **Bubble**: shows `branch` (truncated ~18 chars) + amber dot when dirty;
  hidden when the pane has no repo. Mounted beside `UsageBubble` in the pane
  chrome; same pill styling (`ct-usage-*` conventions).
- **Panel opening**: App.tsx `mainView = "git"` (same lazy-mount pattern as
  memory/mesh), with `gitRepoCwd` state set from the clicked pane. Escape
  closes. No keyboard shortcut in v1.
- **Graph rendering**: hand-rolled SVG per row cell (vscode-git-graph
  approach). `assignLanes(commits)` maintains active-lane array; first
  parent keeps the column, extra parents open lanes, converging parents
  curve into their existing lane (cubic Bézier). Fixed ROW_H/COL_W; lane
  color = `--ct-graph-lane-<n % 8>` CSS vars derived from theme accents.
  300 rows render fine without virtualization; "Load more" appends.
- **Refs chips**: branch/tag/HEAD pills on the row, colored by kind.
- **Diff**: `@git-diff-view/react` (only new dependency; active, GitHub-style
  split/unified, lowlight highlighting, selectable DOM text). Backend patch
  text → its patch parser. Unified by default, split toggle in the toolbar.
- **Selection**: `.ct-git-diff { user-select: text }` overrides the global
  `user-select: none`; commit subjects/shas also selectable in the detail
  pane.
- **Refresh**: refetch graph on panel open and on a manual refresh button;
  no polling in v1 (graph data is heavier than the 5s `useGitInfo` poll).

### Data flow

Pane chrome already polls `useGitInfo(paneId)` → branch/dirty for the
bubble. Clicking the bubble passes the pane's `cwd` up to App, which mounts
GitPanel with that cwd; the panel drives all further fetches itself.

## Error handling

- cwd not a repo → panel shows an empty state ("not a git repository").
- git binary missing → 500 with message surfaced in the panel error strip.
- Patch >1 MB → truncated with a visible "diff truncated" banner row.
- Binary files → backend detects `Binary files differ` and the UI shows a
  placeholder instead of a diff.

## Testing

- Rust: unit tests for the log-format parser (records with/without parents,
  refs classification, weird subjects with newlines excluded by %s), numstat
  parser (renames `old => new`), merge-diff parent selection. Run against a
  fixture repo built in a tempdir via std::process git calls.
- TS: `laneGraph.test.ts` (tsx convention) — linear history, branch+merge,
  octopus merge, two roots; lane compaction; deterministic colors.
- Manual on dev: open panel on the codeterm repo itself (rich history with
  merges), verify rails, refs, diffs, selection, web mirror.

## Rollout

Single PR-sized change on `feat/git-viewer`; no schema, no config, no
migrations. Feature is additive and isolated to the new module + small
App.tsx/pane-chrome touchpoints.
