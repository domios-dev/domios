---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_c62b1313ff854642940b2f25f39eb0b8
handle: transparency-kill
title: Kill remaining status/dominance opacity (§32.3 fully)
status: done
---

# Kill ALL remaining status/dominance opacity on canvas nodes (§32.3 fully)
Andrey at 387% zoom: a "Cancelled" task node is rendered WASHED-OUT / semi-transparent (whole node dimmed). §32.3 = transparency is NEVER a status/hierarchy signal. cloud-fix removed SUBTASK opacity + dominance soft-dim, but CANCELLED/done/non-focused nodes still get an opacity reduction somewhere. FIND every opacity/alpha/fillOpacity tied to status (cancelled/done) or dominance/focus on Canvas nodes and REMOVE it — nodes SOLID at all zooms. Done/cancelled deprioritize SPATIALLY (sink/position), never by fade. grep src/components/Canvas for opacity/alpha/dim/fade. Keep ONLY opacity for genuinely-transient focus pulse (§31) if any. Verify visually: cancelled node is solid.
