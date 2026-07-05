---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_cb6bd7ecc8934ccdb917ba504c83aeac
handle: f1-plan-recall-index-mem-sidecar-ms_cb6bd7ecc8934ccdb917ba504c83aeac
title: decision
---

ORCHESTRATION PATTERN UPGRADE (Andrey): workers now READ their task FROM the plan via CLI (codeterm task show <id> + plan get <pid>), not from an inlined /tmp brief. Full impl spec lives in the TASK BODY (dogfood); the spawn brief is a THIN launcher pointing at it. This truly manages work through the system we build (and rehearses the prompts we'll hardcode). Applied to fresh F1 worker.
