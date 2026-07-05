---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_d7388f654b95451a9025ebd6e3c35471
handle: cloud-left-pull-ms_d7388f654b95451a9025ebd6e3c35471
title: decision
---

§35 comet fold in taskCloud.ts — foldedTheta() mirrors not-done clusters to left half (plan side), done/cancelled to right; activeRanks now ranks all not-done (in_progress innermost, idle next); subtasks drag via unchanged cluster mechanism. testing: 26/26 taskCloud.test + 42/42 layout.test, tsc clean. Pushed feat/worlds-v3-cloud-left @8d0842a9
