Verification stays proportional:
- Before committing Rust, source `build.sh env`; run `CARGO_BUILD_JOBS=4 cargo check -p <crate> --lib` with the owning manifest; add `--tests` for changed tests. Cargo locks shared intermediates; no gateway. Committed integration Clippy uses check-queue. Lane checks stay isolated; full/app builds, dev, suites, packaging and self-install stay in canonical.
- Orchestrator reviews and real-merges lanes, then reruns each wave's focused selectors once in canonical; failures bounce. The broad gate runs once on the release SHA.
- Keep bounds; no higher cap or blind retry after a deadline.
