---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_e708f548f5d348a686a0d2b13f3abb6a
handle: plan-store-coherence-watch-the-sidecar-dir-reconcile-external-changes-ms_e708f548f5d348a686a0d2b13f3abb6a
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/plan-atomic-writes @ 3029f02b
CHANGES:
- NEW src-tauri/src/service/plan/atomic_write.rs: shared helper unique_tmp_path (.{name}.{pid}.{seq}.tmp) + write_atomic (mkdir parents, unique-temp write, sync_all, atomic rename, best-effort temp cleanup on failure). 3 unit tests.
- mod.rs: write_atomic (plan.json) and write_spec_atomic (spec.json) now route through atomic_write::write_atomic; dropped fixed .json.tmp paths.
- body_files.rs: write_markdown_atomic now delegates to shared helper; dropped fixed .md.tmp path.
- stride_log.rs: rewrite_milestones reuses shared helper; removed its private duplicate unique_tmp_path; updated the disjoint-tmp test to a rewrite->read roundtrip + no-leftover-tmp assertion.
BUILD/TYPECHECK/TESTS: cargo fmt --check PASS. Could NOT run cargo test in this cold worktree: build.rs fails (bundled-embed/codeterm-embed-aarch64-apple-darwin missing) even against shared CARGO_TARGET_DIR — an embed-bundle env issue, not a code error. Needs verification on your warm checkout: TEST_CAP_SECS=180 scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::plan::atomic_write  AND  --lib service::plan::stride_log.
FOLLOWUP: swept all non-test writers in service/plan for fixed-temp .tmp/rename/File::create — only the 4 above existed; all migrated. Remaining fs::write hits are test-only (paths.rs marker under cfg(test), skill fixtures). Behavior change: plan.json/spec.json/markdown writes now fsync before rename (durability gain, matches stride_log). None out of scope.
