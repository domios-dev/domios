---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_8be408c0d90243eb9c0f07abcd4641a2
handle: p1c-zero-flag-report
title: 'P1c: zero-flag reporting via pane-identity resolution'
status: done
---

Zero-flag worker context: reports, strides, and status updates resolve plan/task from pane identity — no --plan/--task needed (spec section worker-context). Sequence AFTER p0-report-reliability lands (same CLI report path; take it in that lane as a follow-on to avoid conflicts).

Live incident rationale: a planner pane's CODETERM_TASK_ID pointed at a deleted task after a remove+re-add; reports needed manual --plan/--task. The daemon records task assignment per pane — pane identity alone is sufficient and fresher than exported env.

Steps:
1. Resolution order for agent report / plan record / task status updates from a worker pane: explicit flags > pane env (CODETERM_PLAN_ID/CODETERM_TASK_ID, validated against daemon state — a stale/deleted id is IGNORED with a notice, not trusted) > daemon-side lookup: the pane's currently assigned task (assignment stored at plan task assign/update --pane). Implementation: daemon-side resolution in the agent.report / plan record handlers (src-tauri/src/agent/reports.rs, service/plan mutation entry points), CLI stops requiring flags (cli/src/cmd/agent.rs).
2. Unresolvable (unassigned pane, no valid env): fail LOUDLY listing what was tried (flags/env/assignment) in REPORT NOT SENT framing — never guess, never drop.
3. Ambiguity (pane assigned multiple open tasks): require --task, error names the candidates.
4. Works identically for local and remote (mesh-homed) workers — resolution happens on the OWNING daemon using its authoritative assignment state (composes with p1b auto-homing).
5. Tests: env-stale-but-assigned resolves via daemon; unassigned fails loudly; multi-assignment demands --task; explicit flags always win; remote path resolves on owning peer.
6. Prompts/docs: worker templates + ORCHESTRATION doc drop the "always pass --plan/--task" guidance; state the resolution order + failure modes.

Commands: ./build.sh check; narrowed cargo tests (cli + src-tauri), single runs; live smoke: report from an assigned pane with NO flags and with a deliberately stale env.
Report: resolution matrix implemented, test output, live smoke transcript.
