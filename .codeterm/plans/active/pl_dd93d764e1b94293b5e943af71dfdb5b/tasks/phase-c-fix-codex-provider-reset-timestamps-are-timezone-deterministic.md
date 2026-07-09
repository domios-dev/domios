---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_6e84925f45cd40e0a2293bb48455a066
handle: phase-c-fix-codex-provider-reset-timestamps-are-timezone-deterministic
title: 'Phase C fix: Codex provider reset timestamps are timezone deterministic'
status: done
---

ROG Windows Phase C ran plugin tests via Git Bash. Only plugins/provider-codex/tests/plugin.test.ts failed: session reset keeps local timezone converted to UTC instant and weekly reset keeps local day/month mapping. Errors: expected 1781407020000, got 1781381820000; expected 1784029500000, got 1784004300000.

ROG classified this as a real timezone-coupling bug: the test uses now = Date.parse('2026-06-13T10:00:00-04:00') and expectations with -04:00, but result is seven hours earlier on Europe/Riga host, implying host-local timezone leakage in Codex /status reset parsing or the test. Fix by making reset parsing timezone-explicit/deterministic, or pinning the test timezone if the parser contract is local-to-user. Prefer parser correctness over test masking. No provider-id boundary changes.
