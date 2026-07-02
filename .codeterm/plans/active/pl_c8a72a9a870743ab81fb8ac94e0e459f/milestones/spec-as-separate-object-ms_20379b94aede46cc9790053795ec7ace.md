---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_20379b94aede46cc9790053795ec7ace
handle: spec-as-separate-object-ms_20379b94aede46cc9790053795ec7ace
title: decision
---

CLI MIS-TARGET ROOT CAUSE (Andrey): the installed CLI is GLOBAL (~/.codeterm/bin); a worker ran ./build.sh cli and its socket-discovery now resolves to a DIFFERENT instance (batchtest), so codeterm plan record from the orchestrator hit the wrong daemon (plan not found). Dogfood plan is SAFE on 7686 (direct curl works). FIX NEEDED: CLI must be per-instance OR strictly honor CODETERM_API_PORT/CODETERM_INSTANCE so rebuilding one dev CLI never redirects another. Workaround now: dogfood via curl to 7686.
