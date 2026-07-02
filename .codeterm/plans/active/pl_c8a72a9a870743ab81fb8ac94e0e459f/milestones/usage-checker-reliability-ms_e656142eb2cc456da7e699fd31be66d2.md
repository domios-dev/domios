---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_e656142eb2cc456da7e699fd31be66d2
handle: usage-checker-reliability-ms_e656142eb2cc456da7e699fd31be66d2
title: status_update
---

Accepted usage-checker: integrated Cursor dollars-to-cents usage fix, Codex /status gauge regex widening, and usage probe readiness/warmup/budget adjustments. Verified hot checkout: npx tsx plugins/provider-cursor/tests/plugin.test.ts (141 passed), npx tsx plugins/provider-codex/tests/plugin.test.ts (61 passed), cargo test --manifest-path src-tauri/Cargo.toml usage::probe (3 passed), env CODETERM_API_PORT=7686 ./build.sh check passed. Dev restart still deferred until active workers are drained.
