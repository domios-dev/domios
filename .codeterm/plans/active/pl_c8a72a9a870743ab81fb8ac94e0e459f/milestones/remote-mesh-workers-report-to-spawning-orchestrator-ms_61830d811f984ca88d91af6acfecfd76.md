---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_61830d811f984ca88d91af6acfecfd76
handle: remote-mesh-workers-report-to-spawning-orchestrator-ms_61830d811f984ca88d91af6acfecfd76
title: status_update
---

Accepted after release verification.

Worker report:
- PR #193 merged to master.
- Commits: 874d0216 feature, fe3fdf81 release bump; master/develop synced at ffbf61d5.
- Tags v1.5.6 and release-1.5.6 pushed.
- Feature: `agent spawn --mesh PEER --report-to` gives durable cross-mesh report target; plain remote `agent report` relays to origin orchestrator/General; authenticated origin_peer anti-spoof; sidebar remote-pane ghost nodes via RemoteWorkerRegistry + GET /api/remote-workers + remote-workers-changed.
- Worker claims routing/serde/relay/registry/manifest tests, fmt/clippy, API mock, and Playwright UI passed.

Orchestrator verification:
- git ls-remote verified master/develop and tags.
- GitHub Actions run 28236023367 completed with conclusion=success at https://github.com/rollacode/codeterm/actions/runs/28236023367.
- Mesh identify still reaches eight.

Follow-up split out:
- Prod-kill incident was existing build.sh env inheritance, not the release branch. Separate P0 safety task task_62f626b6f3184b3b95769af8c1505344 was created and closed with build.sh guard integrated locally.
