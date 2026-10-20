# Codex account balance and reset cancellation

Purpose: prove account money/credits are distinct from rate-limit-reset credits and cancellation never consumes one. Entire journey is core; confirming a reset is forbidden.

## Preconditions / owned fixture

Shared DEV setup. Owner explicitly permits a fresh Codex runtime fixture; do not use somebody else's tab. Create an isolated workspace and use `dc agent spawn codex --workspace "$WS" --title Usageprobe --description "Read-only credits QA" --task-file "$PAD/inert.md" --non-user-facing --json`. The inert brief asks for one opaque READY reply, then no tools/messages/account actions. Record tab/session/thread/nonce and actual ready/idle evidence. `dc usage latest --provider codex --json` is read-only; select only runtime_usage whose tab_id AND thread_id match this fixture. A provider-wide snapshot can contain foreign tabs and must not supply their balance to this UI claim.

## Exact actions / evidence

1. `dc --as-tab "$OWNER" plugin browser open http://127.0.0.1:7686/`; authenticate using inherited DEV token directly from process memory over the owned Browser CDP. Select own workspace/tab. Verify visible `[data-tab-id]`; API tab focus alone did not select the corresponding pane in every client.
2. Before opening the bubble install the scoped request recorder with `tools/cdp-owned-page.mjs` and `tools/reset-observer.js`, context phase=install and the owned tab ID. It records only that tab's `/agent/runtime/reset-credits` action/confirmed flag, never headers or request keys. Existing transport source maps this invocation to the DEV REST path. Do not assume an unchanged number alone proves no consume request.
3. Snapshot and click the own `usage ...` button. Re-snapshot and read only dialog `usage details` innerText. Require `Account credits: <raw own-runtime balance>` or the provider's actual unlimited state. Separately require `Reset credits: N available` and next known expiry matching own runtime resets data when supplied. No reset data means that subcase NOT RUN, not fabricated zero.
4. If N>0, click Reset now once, snapshot and inspect screenshot. Require dialog Reset current rate limits?, explanatory one-credit cost, Cancel and Confirm reset. Click Cancel, NEVER Confirm reset. Re-snapshot: confirmation gone, focus back on Reset now, usage dialog still shows same count.
5. Read/restore the recorder (phase=restore); require zero action=redeem requests. Opening may issue action=read. Capture fresh own runtime usage via `dc usage latest --provider codex --json`; require count/credit statuses unchanged and expiry preserved. Save both DOM and request evidence before cleanup. A recorder failure leaves cancellation safety NOT VERIFIED.
6. Copy screenshots to the reporting pad before closing fixture Browser/tab/workspace; those pads disappear with tabs. Close only owned fixtures. No reset, plan change or payment operation is authorized.

PASS: balance label matches own raw balance; resets separate, expiry correct, Cancel closes confirmation, request recorder shows zero consume and runtime count preserved. FAIL: labels conflated, wrong thread's balance, consume on Cancel or count/status changes attributable to this test. NOT RUN: no runtime/credit data, count0 prevents dialog, auth unavailable or missing grant. Account-wide balance can move due to other agents; do not infer this test consumed a reset from money movement.

Joy a73199a03 executed: own Codex31db53e0/thread01a14a91-d472-7c41-8930-21dfaf26ac5d; Account credits54654.4974635000; Reset credits2 available; next expiry1793299641000 (29Oct2026); confirmation opened and Cancel clicked. Scoped recorder captured one read, zero redeem; fresh own runtime still2 credits available. Screenshots usage-credits.png and usage-reset-confirm.png were opened/inspected and copied to host evidence. All fixtures removed. PASS for this run; not a later release proof.

Automation: pure reset state machine already has focused tests; lowest-layer cancellation test asserts no consume call, not prompt wording. Keep this live own-thread/wiring check with request observation. The draft recorder is generalized from the used helper and needs owner-reviewed live revalidation before becoming a gate.
