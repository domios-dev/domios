---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_c1a20804854c48098d230dca548dd2bb
handle: cli-codeterm-instance-codeterm-instance-so-plan-ops-hit-dev-agent-ops-hit-prod-without-per-call-port-override
title: 'CLI: codeterm --instance/CODETERM_INSTANCE so plan ops hit dev, agent ops hit prod without per-call port override'
status: done
tags:
- friction
---

Global codeterm CLI defaults to PROD 7685 with no first-class instance selector. Orchestrator must export CODETERM_API_PORT=7686 + discover dev token per call for plan ops, but keep 7685 for agent spawn/report. Easy to fat-finger. Want: 'codeterm --instance dev <cmd>' or honor CODETERM_INSTANCE to resolve port+token+socket. High dogfood friction.
