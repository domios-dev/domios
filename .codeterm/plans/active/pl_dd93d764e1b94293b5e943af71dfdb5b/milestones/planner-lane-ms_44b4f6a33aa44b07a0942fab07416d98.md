---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_44b4f6a33aa44b07a0942fab07416d98
handle: planner-lane-ms_44b4f6a33aa44b07a0942fab07416d98
title: status_update
---

PARTIAL: Worker-context requirements folded into P1 architecture.
SPEC: new section worker-context (linked to both P0 and P1 waves): (1) mesh token auth MANDATORY for every remote plan operation — full plan/spec/report surface reachable via --mesh, zero auth bypasses, unauthenticated attempts fail loudly; (2) remote workers KNOW they are remote — mesh-origin in lane env/metadata at spawn, plan/report commands auto-route to the owning peer, no manual --mesh for their own plan; (3) zero-flag reporting for local AND remote: resolution order explicit flags > validated pane env (stale/deleted ids ignored with notice) > daemon-side pane-assignment lookup; unresolvable fails loudly listing attempts; ambiguity demands --task naming candidates.
TASKS: p1b-mesh-homing extended to 6 criteria (+auth audit, +auto-context from lane env). NEW p1c-zero-flag-report (task_8be408c0d90243eb9c0f07abcd4641a2, p1, opus): daemon-side identity resolution in agent.report/plan record handlers + CLI drops mandatory flags + prompt/doc updates; sequenced AFTER p0-report-reliability in the SAME lane (shared report-path files). Live-incident rationale recorded: this planner pane's own stale CODETERM_TASK_ID after remove+re-add.
STATE: 17 tasks, coverage 100%, spec links 15/15. CLI note: task update --criterion REPLACES criteria (p1b restored to full set) — minor ergonomics candidate.
NEXT: unchanged dispatch order — p0-report-reliability lane (active) picks up p1c after its diff lands; p1-sidecar-routing to a second rust worker now; p1b after p1 in that lane; t1-t7 stay held until P0+P1 smokes pass.
