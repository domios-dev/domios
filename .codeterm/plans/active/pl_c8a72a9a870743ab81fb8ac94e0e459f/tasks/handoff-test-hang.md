---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_4cd504c5a1c5406ca4b5676eac724b47
handle: handoff-test-hang
title: 'Test hang: handoff reseed_records_pending_row_and_session_start_sends_once >60s (deadlock?)'
status: done
---

service::plan::handoff::tests::reseed_records_pending_row_and_session_start_sends_once HANGS >60s under the full cargo test --lib run (would hang CI ./build.sh test). Unrelated to the plan_routes split (which passes isolated). Likely a deadlock (parking_lot reentrancy class) or a test that waits on a real channel/session. INVESTIGATE: run it isolated with --nocapture under a timeout; check what it waits on (session_start send-once). test_plan: the test passes under a 30s timeout.
