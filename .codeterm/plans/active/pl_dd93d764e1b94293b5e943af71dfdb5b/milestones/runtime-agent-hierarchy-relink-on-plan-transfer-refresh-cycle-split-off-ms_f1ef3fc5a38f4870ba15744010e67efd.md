---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f1ef3fc5a38f4870ba15744010e67efd
handle: runtime-agent-hierarchy-relink-on-plan-transfer-refresh-cycle-split-off-ms_f1ef3fc5a38f4870ba15744010e67efd
title: decision
---

REFLECTION-TRIGGER DESIGN (Planner ruling, binding): do NOT key after_reflection on a magic learning tag=='reflection' — a tag the model must remember is a rule-that-loses-to-friction and rots silently. INVERT: after_reflection fires on 'a learning recorded WHILE the reflection window is pending' — temporal correlation with DAEMON-OWNED state (the reflection silencer already tracks pending/cleared). The prompt needs ZERO protocol awareness; one natural act (recording the learning) both clears the reflection block AND arms the refresh. DELETE the tag path outright.
