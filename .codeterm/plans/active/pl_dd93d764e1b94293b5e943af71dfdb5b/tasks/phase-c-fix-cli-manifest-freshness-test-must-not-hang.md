---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_b68b897f7b99406d8a19386be62ec383
handle: phase-c-fix-cli-manifest-freshness-test-must-not-hang
title: 'Phase C fix: CLI manifest freshness test must not hang'
status: done
---

Phase C exposed that CLI manifest freshness tests can hang until the 90s cap while computing the clap manifest/markdown from the command tree, even after `codeterm cli-manifest --write` refreshed `docs/CLI.md` and `docs/cli-manifest.json`. This is separate from stale docs: generated files were committed and pre-push cargo check/fmt/clippy passed. Fix the freshness test path so it is bounded and completes quickly, or isolate the expensive/hanging introspection dependency. Keep behavior unchanged; this is a test/reliability fix.
