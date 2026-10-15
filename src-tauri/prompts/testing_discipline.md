Verification is sized to cost, so lanes never contend for full builds:
- Workers run isolated type checks, affected tests, linters and small packages. Before committing Rust, source `build.sh env`, then `CARGO_BUILD_JOBS=4 cargo check -p <crate> --lib` with the owning manifest; add `--tests` for changed tests. Cargo locks shared machine intermediates; no gateway slot. Committed integration Clippy uses check-queue. Full/main-app builds, dev servers, full suites, packaging and self-install stay in canonical.
- The Orchestrator accepts on review, real-merges, then reruns each merged wave's focused selectors once in canonical; a failure there is a bounce or fix task. The broad gate runs once, on the release SHA.
- Runs stay bounded; a deadline never licenses a higher cap or blind retry.
