# Mesh inventory, jobs and persisted results

Purpose: prove the owner sees the correct machine/workspace jobs and that scheduled work leaves a durable answer. Core: local manual+scheduled job; read-only mesh inventory when the integration is configured and granted.

## Preconditions / ownership

Shared DEV setup and a real owned agent caller. Owner grants disposable jobs/workspace/Team, small provider budget, and exact known DEV peer identities if mesh is tested. Discover current cron/mesh help and API-INTEGRATIONS/API-SERVICES. Never pair by display name, touch production peer identities or start peer lifecycle operations without explicit grant.

## Jobs exact procedure

1. Create a pad JSON body for `POST /api/crons` using documented fields name,schedule,task,workspace_id,agent,cwd,description,tz,timeout_seconds. The task is a tiny opaque marker report. Use `dev_run --as-tab "$OWNER" node evals/qa/tools/dev-request.mjs POST /api/crons "$PAD/cron.json" "$OUT/create.json"`; body is read inside the process, never `$(cat ...)` or argv. Record returned job ID and actual inherited Team. Use a far-future repeating schedule for manual trigger, plus a separate explicitly scheduled one-shot using the current accepted time syntax. Do not mutate an owner's job.
2. Manual run: `dc --as-tab "$OWNER" cron trigger "$JOB" --json`. Record returned run/tab IDs once. Watch the real run events via an owner-created Blueprint graph (read BLUEPRINTS first); a real run bucket is required. End the waiting turn rather than poll/sleep. A blueprint is a watcher, not proof of the answer.
3. On terminal event, `dc cron history "$JOB" --json`; require the exact run persisted Success and its expected final answer/report, not just exit0. Open Jobs/history in own Browser, click that run and inspect the answer. A one-shot may delete its job/tab; capture persisted run/session evidence before cleanup and verify the supported viewer can still expose the result. A tab404 is not proof of lost persisted history by itself.
4. In the scheduled case prove the scheduler, not a manual trigger, created the exact run; require same persisted success/answer and rendered viewer. Capture one task/system frame and one user launcher only if the actual schema represents them; do not call a user-row count a system-frame count.
5. UI scope: General lists its jobs/global services and excludes foreign workspace cron/shells. Show all opens machine Jobs with Machine=This machine. Workspace Jobs must show only its scope. Use real Browser snapshot/DOM facts and inspected screenshots; hidden mounted tabs do not count.

## Mesh read-only core / extended cases

1. `dc mesh status --json` and `dc mesh peers --json`; pin exact instance/peer IDs, ports and development identity. Stop on unexpected PROD. For the owner-specified DEV peer, `dc --mesh "$PEER" tab list` / discovered services/crons inventory commands remain read-only. Local peer UI opens Services and Crons; offline peer shows honest inventory unavailable rather than empty-success inventory.
2. Remote send/report/paste, pairing and restart require another grant. Reuse `evals/mesh-two-dev`'s explicit direction, origin, exact-peer and own-session evidence contract rather than inventing a second harness. A remote command ACK must end in own-session turn/report on the far machine and the origin UI. For timing parity, correlate streams and rendered events with bounded clock offset; sequential screenshots cannot prove a1s bound.

PASS: exact scheduled/manual persisted results and correct rendered scope; mesh inventory accurately reflects the granted peer state. FAIL: false Success, missing/wrong answer, scope leakage, stale/fictional inventory or origin/session confusion. NOT RUN: no schedule event, remote grant, peer, viewer or persisted answer. Manual Success cannot pass scheduled execution.

Cleanup: save history/view evidence before deleting owned jobs (deletion cascades run logs/history), cancel only owned running runs if needed, then close owned Team/workspace. Do not delete system jobs or foreign services. Mesh pairing/unpairing and token changes are not cleanup implied by a read-only probe.

Seeds: Keel reported manual persisted Browser/API PASS and scheduled API Success/answer PASS, but scheduled far-end viewer and system-frame-once proof remained incomplete. Turing reports local peer/Services/Crons, offline inventory and General/Show-all scope PASS; summary lacks fresh exact run artifacts. Retain those distinctions.

Automation: persisted-run/state models and scope filters are pure unit targets. Runtime-provider matrix row11 and the two-DEV mesh contract offer existing wiring paths; live release acceptance still requires the actual history viewer and far-end result. No looping job poller is added by this catalogue.
