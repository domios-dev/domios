---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_f69be4d46dfb4ce6b3d3d584eb6488bb
handle: cross-daemon-agent-report-prod-pane-dev-plan-pane-not-found
title: Cross-daemon agent report (prod pane → dev plan) 'Pane not found'
status: done
---

agent report --plan/--task fails with RPC 'Pane not found' when the reporting pane belongs to a different daemon than the plan. Make report resolve the caller identity across daemons (or document --pane requirement). Also: installed CLI must be rebuilt (./build.sh cli) after CLI changes, else agents use stale flags — consider auto-rebuild or a version check.
