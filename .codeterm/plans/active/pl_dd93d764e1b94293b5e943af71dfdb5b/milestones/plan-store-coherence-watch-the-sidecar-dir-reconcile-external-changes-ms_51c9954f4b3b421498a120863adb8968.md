---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_51c9954f4b3b421498a120863adb8968
handle: plan-store-coherence-watch-the-sidecar-dir-reconcile-external-changes-ms_51c9954f4b3b421498a120863adb8968
title: status_update
---

IN_REVIEW: STATUS: in_review
BRANCH: lane/plan-store-log-resilience (pushed, commit 44558a94)
CHANGES:
- src-tauri/src/service/plan/stride_log.rs: read_milestones now skips malformed/unreadable/blank JSONL lines with a WARN (per-line, path+line-num diagnostic) instead of returning MalformedLine and making the loader drop the whole plan. Removed the now-dead MalformedLine variant. rewrite_milestones uses a per-writer unique tmp (.<name>.<pid>.<seq>.tmp) + fsync + cleanup-on-failure instead of the shared fixed strides.jsonl.tmp. Appends go through one O_APPEND write_all per line (append_line helper). Added tests: malformed-line-skipped, corrupt_bak_shape (torn whitespace + truncated line = the .corrupt.bak shape), concurrent_appends_never_tear_a_line (8x50 threads), disjoint-tmp-path.
- cli/src/cmd/plan_skill/ledger.rs: append_entry now builds json+\n and does one write_all (was writeln! = 2 syscalls, tearable). Both ledger readers were already tolerant (filter_map .ok()).

ROOT CAUSE: torn strides.jsonl came from rewrite_milestones sharing a FIXED tmp name across processes — prod(7685)+dev(7686) daemons on one .codeterm/plans clobbered the same strides.jsonl.tmp mid-write, then rename published the torn file. Fixed via unique tmp.

BUILD/TYPECHECK/TESTS:
- cli crate: compiled + ledger tests pass (7 tests, exit 0) via shared warm target.
- src-tauri stride_log tests: COULD NOT RUN in this cold worktree — the tauri embed build-script fails (resource 'bundled-embed/codeterm-embed-aarch64-apple-darwin' doesn't exist), which blocks the whole crate before Rust compiles. Documented swept-embed infra issue, not a code error. Orchestrator gate command (warm checkout):
  CARGO_TARGET_DIR=.../codeterm/src-tauri/target scripts/test-capped.sh --manifest-path src-tauri/Cargo.toml --lib service::plan::stride_log

FOLLOWUP:
- Same fixed-tmp collision pattern still exists in write_atomic (plan.json, mod.rs:122), write_spec_atomic (spec.json, mod.rs:158), write_markdown_atomic (body_files.rs:674). A torn plan.json makes an ENTIRE plan vanish (the original incident class). Out of my logging-scope boundary but recommend a shared unique-tmp atomic-write helper across all three. Left untouched to avoid touching mod.rs write paths other lanes may own.
- Append safety relies on POSIX O_APPEND atomicity for a single write_all; sufficient for torn-line prevention. No cross-process flock added (portability/Windows) — the daemon path funnels through rewrite (now unique-tmp) anyway.
- The task's own acceptance criteria describe the broader FS-watch/reconcile scope; this lane delivered only the log-resilience sub-scope from my worker brief.
