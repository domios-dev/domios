---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0ff0aa0cd6fd46fb8044e5f8a5e72d15
handle: gh-203-windows-startup-spawns-duplicate-shell-conhost-storm-ms_0ff0aa0cd6fd46fb8044e5f8a5e72d15
title: Windows dev-lock/dev-cleanup class fixed and integrated in origin/feat/canvas-updates @ 313351f8. ROG evidence
---

lint PASS, health/CDP PASS, smoke no longer hits tauri-build os error 32 or ECONNREFUSED cascade; dev stayed healthy. Smoke still fails on real uiSession setup timeout + missing lmstudio plugin, so GH #203/Windows regression remains open for next bounded shard.
