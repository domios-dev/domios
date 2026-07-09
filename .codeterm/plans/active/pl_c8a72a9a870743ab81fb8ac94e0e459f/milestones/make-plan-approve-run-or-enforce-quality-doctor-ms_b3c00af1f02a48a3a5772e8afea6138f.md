---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_b3c00af1f02a48a3a5772e8afea6138f
handle: make-plan-approve-run-or-enforce-quality-doctor-ms_b3c00af1f02a48a3a5772e8afea6138f
title: status_update
---

codeterm plan approve now runs local quality doctor preflight before setting approved mode and blocks with exact doctor/approve recovery commands when quality fails. Gates: cargo test cli plan_doctor 9/9, plan_planning_gate_tests 3/3, cmd::manifest 5/5.
