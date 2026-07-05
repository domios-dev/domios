---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_f365c202c4404fe6b4f5735b1357256f
handle: test-hygiene-fix-pre-existing-failures-flagged-by-remote-lanes
title: 'Test hygiene: fix pre-existing failures flagged by remote lanes'
status: done
---

Two independent remote lanes (Dali@eight) hit the same pre-existing, unrelated test failures while running full gates — they cost every lane triage time to classify as not-mine. Fix all three: (1) cli plan renderer tests failing on stale include_str fixtures; (2) orchestration_prompts drift test failing (prompt constants moved); (3) search_index vector-count test env-dependent (flaky by environment — pin the env or make the assertion environment-independent). A green baseline is a prerequisite for trusting any lane's full-gate run.
