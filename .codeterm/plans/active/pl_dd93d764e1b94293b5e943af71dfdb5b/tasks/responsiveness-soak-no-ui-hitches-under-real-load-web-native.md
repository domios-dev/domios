---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_acb999e3642343e7bb196d7f9370bfbc
handle: responsiveness-soak-no-ui-hitches-under-real-load-web-native
title: 'Responsiveness soak: no UI hitches under real load (web + native)'
status: done
---

The closing verification of Wave 26, after busy-states, boot-stage events, lock hygiene, and the spawn-latency merge land. Scenario soak on dev, BOTH web mode and native: while three agents spawn in parallel, continuously switch tabs, open the plan board, scroll the guide — capture a performance trace (Playwright tracing / WebKit timeline where feasible) and the dev log. PASS conditions: no frame gap > 250ms during the soak (trace evidence); zero SLOW-IPC entries attributable to the interaction window; zero LOCK-HOLD warnings; every waiting surface showed a busy state (spot-check screenshots); web and native behave equivalently (same scenario both, deltas explained or fixed). Findings = bounces to owning lanes, not inline fixes.
