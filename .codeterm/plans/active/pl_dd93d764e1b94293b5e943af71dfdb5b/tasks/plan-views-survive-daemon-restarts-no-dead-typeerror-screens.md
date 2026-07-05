---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_9076ae9e5c96496bb94a9fc56d362a9a
handle: plan-views-survive-daemon-restarts-no-dead-typeerror-screens
title: 'Plan views: survive daemon restarts (no dead TypeError screens)'
status: done
---

User sees 'TypeError: Load failed' rendered red in the plan view area (screenshot, round-12). Diagnosis: that string is WKWebView's generic fetch network-failure message — the plan data fetch died mid-flight, almost always because the dev daemon restarted (we restarted it twice within the hour) or was briefly unreachable. The bug is resilience, not data: the view renders the raw exception PERMANENTLY with no retry, so after every daemon restart any open plan tab is dead until manual reload. FIX: (1) all plan-view data fetching gets auto-retry with exponential backoff (250ms -> 5s cap) while showing a quiet inline 'reconnecting…' state — never a raw exception string anywhere in plan surfaces; (2) subscribe to the existing daemon-health/connection signal (the frontend knows when the bridge reconnects) and refetch immediately on recovery; (3) grep-sweep plan-view components for other raw err.message renders and route them through one shared ErrorState component with retry; (4) same treatment for the sidebar plan entry badge fetch if it shares the path.
