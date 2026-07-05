---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ee8c1fbbd9634a4ba5e9a167dc1b4b4a
handle: daemon-perf-full-screen-vt100-hash-on-every-pty-chunk-melts-cpu-under-st-ms_ee8c1fbbd9634a4ba5e9a167dc1b4b4a
title: status_update
---

IN_REVIEW: STATUS: in_review (FOLLOW-UP LANE, same perf task)
BRANCH: lane/perf2-composer-poll @ 1777cef8 (base be485acc — includes the merged perf pair)
CHANGES: third instance of the visibility-ignored-work class, found by interval sweep: ChatComposer polled listDelayedMessages every 3s for EVERY mounted composer (background panes included) despite (a) the delayed-send-changed event subscription already driving updates and (b) isActive being plumbed into the component and ignored by the effect — this is the sqlite-read churn from the CPU profile. Fix: interval only when isActive, 3s->10s (events remain the primary update path; poll is countdown-drift insurance), event listener stays always-on (cheap, keeps background pill state fresh on real changes).
TESTS RUN: none (order: write, don't run). No new test file — the change is a one-line conditional on an existing prop; recommend QA assert: mounted composer with isActive=false issues zero list_delayed_messages calls over 15s (the seam is the invoke mock).
GATES REQUESTED: tsc; ChatComposer/PaneSessionChat suites; live drill: 5 background chat panes -> zero periodic delayed-message IPC in [SLOW-IPC] logs.
SWEEP VERDICTS for the remaining interval sites (audited, no action): webEvents/outputStream watchdogs = global singletons ok; multiplexer saveInterval = one global ok; useShellIntegration = self-terminating retry ok; SecretBackendDetail 5s = mounts only in settings detail ok; BroadcastToggle = mesh panel only ok; useTerminalResize healTimer 2s/pane reads clientWidth with a zero-guard — minor layout-flush cost on hidden panes, NOT fixed (risk/benefit under time pressure), noted for the perf backlog.
