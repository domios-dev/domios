---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_2494b228d5584f70a35be6c31954f19f
handle: 33-1-spike-git-merge-union-for-append-only-stride-logs-ms_2494b228d5584f70a35be6c31954f19f
title: status_update
---

Accepted and integrated §33.1 merge=union prerequisite spike.

Changes:
- Added `.codeterm/plans/**/strides.jsonl merge=union` to `.gitattributes` for future append-only plan stride/event logs.
- Added `scripts/test-plan-strides-union-merge.sh`, a temp-git regression that creates concurrent branch EOF appends to a plan `strides.jsonl`, merges with git's built-in union driver, asserts both branch events survive exactly once, and asserts deterministic fold/dedup order by `(ts, id)`.

Orchestrator gates:
- scripts/test-plan-strides-union-merge.sh: passed.
- git diff --check: passed.
- bash scripts/check-file-sizes.sh: reports unrelated pre-existing oversized files; touched files are 89 and 11 lines.
- CODETERM_API_PORT=7686 ./build.sh check: passed.

Residual risk:
- This is only the §33 prerequisite spike/rule/test. The parent mergeable-storage implementation remains open for explode migration, per-entity write path, post-merge reindex, and compaction.
