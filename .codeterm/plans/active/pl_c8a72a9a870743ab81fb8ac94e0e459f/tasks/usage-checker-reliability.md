---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_6314a3b4e6ee4a6e99ed30ab8afc7d46
handle: usage-checker-reliability
title: 'Usage checker reliability: Cursor remaining + Codex status timeout'
status: done
---

Investigate and fix usage checker reliability for Cursor and Codex. Observed dogfood issues: Cursor poorly determines how much usage is left; Codex /status checker/probe is slow and often times out, suggesting overly long timers, wrong readiness detection, or fragile parsing. Scope: inspect usage tracking/probe implementation for Cursor and Codex providers, timeouts, parsing, and UI surfacing. Make a narrow fix or report a precise diagnosis if the provider output is insufficient.
