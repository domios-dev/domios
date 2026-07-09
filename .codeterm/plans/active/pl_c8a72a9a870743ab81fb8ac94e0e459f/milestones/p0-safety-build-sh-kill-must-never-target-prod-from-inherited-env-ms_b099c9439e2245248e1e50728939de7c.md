---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_b099c9439e2245248e1e50728939de7c
handle: p0-safety-build-sh-kill-must-never-target-prod-from-inherited-env-ms_b099c9439e2245248e1e50728939de7c
title: status_update
---

Accepted with integration adaptation.

Changes integrated into hot checkout:
- build.sh canonical/no-CODETERM_INSTANCE path now hard-pins CT_PORT=7686, ignoring inherited CODETERM_API_PORT=7685 from prod panes.
- Explicit/named instance CODETERM_API_PORT=7685 is refused during setup.
- kill_dev also refuses CT_PORT=7685 as a second guard.
- Added scripts/check-build-kill-safety.sh for focused regression coverage.

Verification run by orchestrator:
- bash -n build.sh: passed
- bash -n scripts/check-build-kill-safety.sh: passed
- CODETERM_API_PORT=7685 ./build.sh env: status 0, no prod-port refusal, canonical path resolves safely to dev env
- CODETERM_INSTANCE=prod CODETERM_API_PORT=7685 ./build.sh env: status 1 with "refusing to target prod CodeTerm port 7685"
- git diff --check: passed

Skipped in hot checkout:
- Did not run scripts/check-build-kill-safety.sh in the hot checkout because it intentionally exercises ./build.sh kill and may stop the active dev listener on 7686. Worker already ran the full script in the lane.
