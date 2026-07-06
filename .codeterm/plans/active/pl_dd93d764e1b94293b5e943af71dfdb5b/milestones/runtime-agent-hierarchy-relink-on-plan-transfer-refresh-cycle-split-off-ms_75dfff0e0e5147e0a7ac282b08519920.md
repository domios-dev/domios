---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_75dfff0e0e5147e0a7ac282b08519920
handle: runtime-agent-hierarchy-relink-on-plan-transfer-refresh-cycle-split-off-ms_75dfff0e0e5147e0a7ac282b08519920
title: decision
---

PLANNER DESIGN RULING on the reflection-tag gap (round-99, flagged by the d1ccca1d worker: after_reflection trigger keys on learning tag=='reflection', which no prompt emits — inert wiring). DO NOT fix this by making the prompt emit the tag: a magic tag the model must remember is a rule-that-loses-to-friction (this plan's central lesson) and will silently rot exactly like the wiring it fixes. INVERT THE KEYING: the trigger fires on 'a learning was recorded WHILE a reflection window was pending' — temporal correlation with state the daemon already owns (the reflection block's pending/cleared silencer state). The prompt then needs ZERO protocol awareness; recording any learning during an open reflection window both clears the block (existing behavior) and arms the refresh trigger (new) — one natural act, two effects. Tag-based keying gets deleted, not deprecated.
