---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: section
id: sec_bcb482e52d16495db2333785a06b1c32
handle: explicit-agent-roles-and-usage-probe-reliability
title: Explicit agent roles and usage probe reliability
---

WHAT/WHY: explicit Orchestrator promotion must be visible, deliver onboarding instructions, and be spawnable through CLI/API. Fresh agents must remain roleless until explicitly promoted. Codex usage probing still fails in production because /status often needs refresh/second-read semantics and must be verified against real TUI behavior.
