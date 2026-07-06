---
plan_id: pl_f75de79aca0c44f3b5af8a4e1ca4a35a
kind: task
id: task_11713016553341179b9e020ca7438bfe
handle: coverage-map-from-headers-flake-quarantine-policy-traces-on-red
title: Coverage map from headers + flake quarantine policy + traces on red
status: pending
---

Executes P7+P8. (1) COVERAGE MAP: script parses the (now-mandatory) spec headers into a surface->specs markdown map; zero-coverage surfaces listed loudly; regenerated in CI. (2) FLAKE POLICY: @quarantine tag — flaky test quarantined + task filed, never retried-to-green; reflection window counts quarantine size. (3) traces attached to every red QA report stride.