---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7004869b8fde48adaa1417d51649f63f
handle: p1-sidecar-routing-ms_7004869b8fde48adaa1417d51649f63f
title: status_update
---

STATUS: done
CHANGES: none (review only)
BUILD/TYPECHECK/TESTS: not run (static review per spec)
FOLLOWUP:

=== ADVERSARIAL REVIEW: P1 sidecar routing lane ===

VERDICT: needs minor changes — claim substantially holds, one real gap found

---

FINDINGS (ordered by severity):

[MEDIUM] delete_plan_units uses raw project_path, not canonical
  File: src-tauri/src/service/plan/search_index.rs:271-274
  Every other search-index operation (mirror_plan, mirror_spec) was updated to use
  plan_project_root()/spec_project_root() helpers. delete_plan_units was not:
    pub fn delete_plan_units(plan: &Plan) -> Result<(), String> {
        delete_units(Path::new(&plan.project_path), ...)
    }
  Under new code project_path == canonical_project_path so this is a silent no-op bug;
  if they ever diverge (manual JSON edit, stale load, race), deletion opens the wrong
  SQLite and silently succeeds without actually removing the units.
  Caller: mod.rs:988 (plan delete path). Fix: replace with plan_project_root(plan).

[MEDIUM] normalize_plan_paths re-resolves canonical_project_path via project_root() on reload
  File: src-tauri/src/service/plan/paths.rs:148-151, mod.rs:838-839
  reload_from_disk derives project_root from plan.project_path (correct), but
  normalize_plan_paths then also calls project_root(canonical_project_path). If
  canonical_project_path points to a deleted linked worktree, git commands fail and the
  path stays as the stale linked path. This only affects pre-existing corrupted data —
  new code never creates this state — but it means canonical_project_path on reload is
  not idempotent when the linked worktree directory is gone.

[LOW] reindex_project_index filters by plan.project_path, not canonical_project_path
  File: src-tauri/src/service/plan/search_index.rs:121, 126
  .filter(|plan| plan.project_path == root_text) — root_text is derived from
  project_root() (always main worktree). Under new code project_path IS root_text, so
  the filter works. Latent inconsistency only.

[LOW] inspect_stray_sidecars canonical_project_path() falls back to project_path for old plans
  File: cli/src/cmd/plan_doctor.rs:178-185
  For old plans without canonical_project_path field, falls back to project_path. If an
  old plan had project_path = /linked/worktree (pre-fix), doctor would treat the linked
  worktree as canonical and the main worktree copy as stray — exactly backwards. In
  practice those old plans live in the LINKED worktree's .codeterm, which doctor never
  scans (it always resolves to main worktree). False positive risk is theoretical only.

[LOW] plan_stem in doctor doesn't sanitize id for path traversal
  File: cli/src/cmd/plan_doctor.rs:207-214
  Plan IDs follow pl_<uuid_hex> format, so unreachable via normal code paths. Would
  require a maliciously crafted local plan.json. No live risk.

[INFO] Six redundant canonical_project_path empty-check sites
  mod.rs:360-361, 396-397, 677-678, 712-713; mutations.rs:305-306; spec.rs:110-111
  Defense in depth, but if project_path is wrong, canonical silently inherits the bug.
  Should normalize once at load/create time, not at every write site.

---

ANGLES CHECKED:

1. Creation default: PASS. create_plan and create_spec both call paths::project_root()
   before Plan::new/Spec::new, which now always resolves to main worktree (linked
   worktree .codeterm check removed). Both project_path and canonical_project_path are
   set to main worktree path at construction. Test in canonical_path_tests.rs confirms.

2. Persistence writers: MOSTLY PASS. write_atomic uses plan_sidecar_root(), body_files
   uses canonical_project_path() helper, spec_files uses canonical_project_path() helper,
   search_index mirror_plan/mirror_spec use plan_project_root()/spec_project_root().
   GAP: delete_plan_units uses raw project_path (see MEDIUM finding above).

3. Read/reload: PASS. load_file_into_index receives project_root from scan_project which
   calls paths::project_root (always main). reload_from_disk derives root from
   plan.project_path and then calls normalize_plan_paths — correct for all new plans.
   For old plans with stale canonical_project_path the re-resolution is slightly fragile
   (see MEDIUM finding) but not a live bug.

4. CLI/doctor: PASS with caveat. worktree_roots correctly shells out to git worktree list
   --porcelain. Stray detection correctly filters canonical project and checks linked
   worktrees. Test storage_warns_about_stray_linked_worktree_sidecar_copy is present and
   covers the primary case. CLI project_root() in plan_local.rs now calls
   main_worktree_root via git rev-parse --git-common-dir, so doctor is always pointed at
   main worktree.

5. Compatibility: PASS. Old JSON with empty canonical_project_path gets defaulted to
   project_path at every write site (and normalize_plan_paths at load time). The
   #[serde(default)] attribute ensures deserialization from old JSON succeeds cleanly.
   Test in types.rs:635 verifies old JSON deserializes with empty canonical_project_path.

6. Data risk: PASS. No path traversal via plan IDs (pl_<uuid_hex>). No deletion of
   wrong project sidecars (remove_body_dir uses canonical helper, remove_file uses the
   indexed path). No watcher loop: watcher is pointed at canonical .codeterm/plans
   (set up via ensure_plan_project_loaded which calls project_root first).

---

GATES RUN:
  git log --oneline 99e6711d..HEAD
  git diff 99e6711d..HEAD --name-only
  git diff 99e6711d..HEAD on all 15 changed files (static read)
  cat src-tauri/src/service/plan/paths.rs
  cat src-tauri/src/service/plan/canonical_path_tests.rs
  cat src-tauri/src/service/plan/watcher.rs
  sed -n targeted reads on mod.rs, search_index.rs, mutations.rs, spec.rs, batch.rs
  grep for project_path usages bypassing canonical helpers across all plan/ files
