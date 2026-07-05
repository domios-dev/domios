---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_de672778581841509a32d36f48a09f43
handle: usage-limit-reset-delay-detector
title: Usage limit reset detector and auto-delay scheduling
status: done
---

Detect provider limit/reset messages for Codex and Claude usage states. For Codex, parse the reset-time message shown in the uploaded screenshot /Users/rollacode/.codeterm/uploads/33daede5-______________2026_06_25___22.38.30.heic and convert it into a next-available timestamp. For Claude, reuse/improve the existing detector but avoid blocking picker flows. Surface the reset time in usage/status and offer or create a delayed orchestrator reminder when a worker is limited.
