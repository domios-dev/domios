---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: orphan_markdown
id: tasks/dev-cache-cleanup-automation.md
handle: dev-cache-cleanup-automation
title: tasks/dev-cache-cleanup-automation.md
---
Integrate or finish the build/dev cache cleanup harness: cargo targets/incremental/cargo-sweep, orphan target dirs, pnpm/npm/pip/go/brew cache cleanup, Playwright reporting, Docker prune where daemon is available. Keep official package-manager cleanup commands where possible. Dry-run default, --apply explicit, configurable KEEP_DAYS, no app state deletion.
