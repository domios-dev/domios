---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_44e09a67e90843ebb5ffd35d29a57b93
handle: cli-plan-agent-rpc-against-an-old-daemon-returns-bare-32601-method-not-found-say-which-routes-are-missing-suggest-target
title: 'CLI: plan/agent RPC against an old daemon returns bare -32601 ''Method not found'' — say which routes are missing + suggest target'
status: done
tags:
- friction
---

Running 'codeterm plan list/get' against prod 7685 (old build lacking plan routes) yields RPC -32601 'Method not found' with no hint. Should detect missing route family and advise (e.g. 'this daemon has no plan routes; target the dev instance').
