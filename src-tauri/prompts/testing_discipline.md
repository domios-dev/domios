Verification stays proportional:
- Before committing Rust, run `./build.sh lane-check <crate> [--tests]` on every platform; add `--tests` for changed tests. Its subshell selects shared intermediates with four Cargo jobs and Cargo locking; no gateway. Committed integration Clippy uses check-queue. Lane checks stay isolated; full/app builds, dev, suites, packaging and self-install stay in canonical.
- Orchestrator reviews and real-merges lanes, then reruns each wave's focused selectors once in canonical; failures bounce. The broad gate runs once on the release SHA.
- Keep bounds; no higher cap or blind retry after a deadline.
