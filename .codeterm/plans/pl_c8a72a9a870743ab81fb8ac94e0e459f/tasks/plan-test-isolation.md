---
tags:
- tests
---

# Fix plan-test parallel isolation (central project_root collision)

After feat/central-project merged, `cargo test --lib service::plan` (default PARALLEL threads) fails 1/66:
`service::plan::tests::reload_from_disk_does_not_overwrite_newer_in_flight_plan_mutation` panics at tests.rs:122 (assert_eq updated_at/title). It PASSES isolated and PASSES serial (--test-threads=1, 66/66). So it is a TEST ISOLATION bug, not a logic regression: central makes project_root resolve to the single main worktree, so plan tests that don't isolate their storage now write to a SHARED path and clobber each other under parallelism.

FIX: make each plan test use an isolated project root / temp store (unique temp dir per test, or override the project-root resolver per test), OR mark the colliding tests #[serial] (serial_test) if isolation is impractical. Prefer real isolation (unique temp project root) over serializing — serializing hides the shared-state coupling. Verify: `cargo test --lib service::plan` GREEN under default parallel threads.
