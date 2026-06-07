# Git Viewer — Implementation Plan

Spec: `docs/superpowers/specs/2026-06-07-git-viewer-design.md`
Branch: `feat/git-viewer`

## Task 1 — Backend git routes (Rust)

Files: `src-tauri/src/api/git_routes.rs` (new), `src-tauri/src/api/mod.rs`,
`src-tauri/src/api/routes.rs`.

1. `git_routes.rs`: `run_git(cwd, args, timeout 10s)` helper via
   `std::process::Command` (`--no-pager`, output cap 1 MB, no shell).
2. `git_graph` handler: log format from the spec, `\x1e`/`\x1f` parsing,
   refs classification (`refs/heads/`→branch, `refs/remotes/`→remote,
   `refs/tags/`→tag, `HEAD ->`→head), plus `head_sha`, `branch`, `dirty`.
3. `git_commit_files`: numstat parsing incl. renames (`old => new`, `R100`).
4. `git_commit_diff`: first-parent rule for merges; root-commit fallback;
   binary detection; 1 MB cap with `\n<<TRUNCATED>>` marker; plain-text
   response.
5. Auth on every route; 404 for non-repo cwd; tests with a tempdir fixture
   repo (init → commits → branch → merge → rename) driven by git CLI.

Verify: `./build.sh check`, `cargo test api::git_routes`.

## Task 2 — Frontend panel (React/TS)

Files: `src/components/GitPanel/*` (new), `src/App.tsx`,
pane chrome where `UsageBubble` mounts, `package.json`
(`@git-diff-view/react`).

1. `gitApi.ts`: IS_WEB-aware fetchers mirroring `memoryApi.ts`.
2. `laneGraph.ts`: `assignLanes()` per spec + `laneGraph.test.ts` (tsx
   runner): linear, branch+merge, octopus, two roots, lane compaction.
3. `GraphView.tsx`: row list (300/page, Load more) with SVG rail cell —
   dots, pass-through verticals, Bézier parent edges, ref chips, relative
   dates; row click → select commit.
4. `CommitDetail.tsx`: file list (+/- stats, rename arrows) → file click
   loads patch → `@git-diff-view/react` unified/split with lowlight;
   `.ct-git-diff { user-select: text }`.
5. `GitBubble.tsx`: branch pill + dirty dot from existing `useGitInfo`
   data; click bubbles `cwd` up.
6. `GitPanel.tsx`: MemoryPanel-pattern shell (toolbar: repo path, branch,
   refresh, split toggle, close; Escape closes).
7. `App.tsx`: `mainView "git"` + `gitRepoCwd` state, lazy mount like
   memory/mesh.

Verify: `npx tsc --noEmit` (no new errors), `npx tsx laneGraph.test.ts`.

## Task 3 — Integration + verification on dev

1. `./build.sh dev` restart; open panel against the codeterm repo (merges,
   renames present in history).
2. curl-verify all three endpoints (shapes per spec).
3. Web mirror smoke check (IS_WEB paths).
4. Commit(s) on `feat/git-viewer` with conventional messages.

## Risks

- `@git-diff-view/react` patch-parser input quirks → fallback: parse hunks
  ourselves into its structured input (documented API).
- Octopus merges are rare but must not crash the lane algorithm (test).
- Pane chrome layout shift when two bubbles render — keep bubble compact,
  hide entirely when no repo.
