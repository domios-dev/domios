---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_baeb507c677044ee83ca06d3492c7c7c
handle: terminal-reduce-pane-mount-xterm-longtasks-from-responsiveness-soak-ms_baeb507c677044ee83ca06d3492c7c7c
title: status_update
---

Accepted and merged in 19f1af77, carried by current head 5e679362. Terminal xterm init is frame-staggered so parallel pane mounts do not stack into a >250ms frame. Focused tests and tsc passed; follow-up native soak remains tracked on the responsiveness-soak review gate.
