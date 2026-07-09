---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_1bac325e8a964022b125a8be7a0e553f
handle: lane-observability-p1-working-on-row-labeled-fields-recording-discipline-ms_1bac325e8a964022b125a8be7a0e553f
title: decision
---

OBSERVABILITY DESIGN NOTE (round-58): split rationale — P1 (this task) is display-from-existing-fields + population discipline + watcher hygiene, shippable before release and fixes the actual root cause (11 of 13 in_progress tasks pane-less = recording, not rendering, is broken); P2 (detail lane panel, mesh focus/resume, any schema delta) explicitly gated on P1's fields being populated. Schema audit verdict: assigned_pane_id/peer/lane/model/status_changed_at/execution_attempts all exist — requirement 7 (data-model changes) is NOT needed for P1 and mostly not for P2 (last_activity_at only if stride-derivation is slow; watcher registry only if title-convention derivation fails). Puccini's requirement 6 (daemon auto-close of watchers) deliberately deferred: the self-close command tail + watch: title convention covers it without a registry; revisit only if watchers leak in practice.
