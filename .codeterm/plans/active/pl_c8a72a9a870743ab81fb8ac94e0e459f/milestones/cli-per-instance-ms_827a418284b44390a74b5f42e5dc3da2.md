---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_827a418284b44390a74b5f42e5dc3da2
handle: cli-per-instance-ms_827a418284b44390a74b5f42e5dc3da2
title: decision
---

ANDREY ROOT-CAUSE: installed CLI is GLOBAL (~/.codeterm/bin); a worker ./build.sh cli rebuilt it and its socket-discovery now resolves to a DIFFERENT instance (batchtest), so orchestrator 'codeterm plan record' hit the wrong daemon. FIX: CLI per-dev-instance location OR strictly honor CODETERM_API_PORT/CODETERM_INSTANCE in socket-discovery so rebuilding one dev CLI never hijacks another. Sibling of the isolated-dev-in-worktree task. Workaround: dogfood via curl to the right port.
