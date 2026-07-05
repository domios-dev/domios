---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_acf9c670f69147c19786bcbb4ab0670c
handle: worlds-v2-ms_acf9c670f69147c19786bcbb4ab0670c
title: review
---

CONFIRMED batch.rs apply_batch atomicity violation — UpsertSpec/UpsertSpecSection/SetSpecSectionBody/CreatePlanFromSpec call store methods that persist immediately, NOT into the rolled-back working clone; later op failure rolls back plan but leaks created spec/plan. batch_is_atomic test only covers task ops.
