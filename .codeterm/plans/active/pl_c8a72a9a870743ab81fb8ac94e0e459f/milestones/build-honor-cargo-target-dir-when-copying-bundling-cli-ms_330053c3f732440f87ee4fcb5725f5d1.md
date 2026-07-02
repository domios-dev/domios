---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_330053c3f732440f87ee4fcb5725f5d1
handle: build-honor-cargo-target-dir-when-copying-bundling-cli-ms_330053c3f732440f87ee4fcb5725f5d1
title: status_update
---

STATUS: in_review
CHANGES: build.sh; scripts/bundle-cli.js; scripts/test-cli-target-dir-paths.sh; commit 684b11ad pushed to origin/lane/cli-target-dir
BUILD/TYPECHECK/TESTS: ran bash -n build.sh; bash -n scripts/test-cli-target-dir-paths.sh; node --check scripts/bundle-cli.js; scripts/test-cli-target-dir-paths.sh. Did not run cargo/build gates per worker constraints.
FOLLOWUP: Orchestrator should run warm integration gate for CARGO_TARGET_DIR + CARGO_BUILD_TARGET actual cargo output, e.g. build CLI into an alternate target dir then run ./build.sh cli and CODETERM_DEV=1 node scripts/bundle-cli.js to confirm copied binary provenance.
