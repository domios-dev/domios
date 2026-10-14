Verification is sized to cost, so lanes never contend for full builds:
- A Worker may run isolated checks in its lane: a type check, affected tests, a linter, a small standalone package, via the build queue (`./build.sh check-queue` for committed Rust). A whole-project or main-app build is a full rebuild left to canonical; lanes start no dev servers and run no full suites, packaging or self-install.
- The Orchestrator accepts on review, real-merges, then reruns each merged wave's focused selectors once in canonical; a failure there is a bounce or fix task. The broad gate runs once, on the release SHA.
- Runs stay bounded; a deadline never licenses a higher cap or blind retry.
