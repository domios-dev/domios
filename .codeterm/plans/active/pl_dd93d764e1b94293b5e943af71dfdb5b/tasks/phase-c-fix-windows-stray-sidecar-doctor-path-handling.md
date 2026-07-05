---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_263a31d169b94320a2dbc1c07ed4a0f2
handle: phase-c-fix-windows-stray-sidecar-doctor-path-handling
title: 'Phase C fix: Windows stray-sidecar doctor path handling'
status: done
---

ROG Windows Phase C at exact integration SHA ce525b518c1edbd535106c630357de43e72342fa failed cmd::plan_doctor::tests::storage_warns_about_stray_linked_worktree_sidecar_copy. The assertion expected finding kind stray_sidecar_copy, with detail containing the canonical main path and path containing the linked worktree path.

Debug Windows path/worktree normalization in the plan_doctor stray sidecar detection. Preserve the intended invariant: canonical plan sidecars live under the main project, linked worktree sidecar copies are warned. Fix the test/implementation so it is deterministic on Windows and Unix, not by loosening away the invariant.
