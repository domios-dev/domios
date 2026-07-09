---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_79e62500bf95492f9115a86d2da127cb
handle: stabilization-pass-reconnecting-banner-hysteresis-jank-sweep
title: 'Stabilization pass: reconnecting-banner hysteresis + jank sweep'
status: done
---

## What

Stabilization pass (round-26): the board flashes "Reconnecting…" every ~500ms while the API returns steady 200s (verified: 6 consecutive 200 probes during the flap — evidence that the banner logic, not the connection, is broken). Plus a general jank sweep — the user keeps catching one flicker after another; hunt the class, not just this instance.

## Item 1 — the reconnecting flap (regression from bcf638f8)

The resilience layer shows its transient state during HEALTHY in-flight refetches. Fix the state machine with hysteresis:
- "Reconnecting…" renders ONLY after >=2 consecutive real failures (network error or 5xx; a single blip stays silent) OR an explicit bridge-down signal; clears on first success.
- In-flight refetch over healthy data renders NOTHING new (stale-while-revalidate is silent by design).
- Audit the refetch cadence while in there: if anything polls at sub-second intervals, replace with event-driven (plans-loaded/plans-changed) + a slow fallback poll (>=30s). Sub-second HTTP polling is a bug on its own.

## Item 2 — jank sweep (the class)

With React Profiler + the soak scenario, sweep the main surfaces (board/list/canvas, sidebar, chat) for: state banners that flap, remount storms (component identity churn — the sidebar sort bug was one), layout shifts from async content, transitions firing on non-user-initiated changes. Enumerate every hit in the report; fix the cheap ones in this lane, file the rest with evidence.

## Executable test plan

1. Playwright: healthy 60s session on the board — assert the reconnecting banner NEVER appears (testid reconnecting-banner absent throughout; poll DOM every 500ms).
2. Playwright with route interception: fail /api/plans twice -> banner appears; restore -> banner clears on next success; single failure -> no banner (hysteresis proof).
3. Unit: the failure-count state machine (0/1/2 failures x recovery transitions).
4. Profiler evidence: before/after commit counts on the board over 30s idle — after must show near-zero re-renders while idle (numbers in report).
