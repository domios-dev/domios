---
plan_id: pl_2e932e82ad614cdd9f08072480b0857e
kind: milestone
id: ms_b059ae0c1b0f483f99790e0c812eb9c7
handle: s1-spike-minimal-substrate-board-task-cards-zoom-crisp-focus-by-id-ms_b059ae0c1b0f483f99790e0c812eb9c7
title: decision
---

M0+M1 done. Flag canvas.substrateV2 via localStorage (new substrateFlag.ts, default off, read-once cached) - existing canvas settings store is a React context for layout constants needing a Provider+settings.ts edit (outside file boundary), so used the lightest localStorage fallback. TaskCard gated: off=unchanged crispScale wrapper, on=pure DOM card(style) ignoring crispScale. No prop/signature change. tsc clean (7 pre-existing plugin-sdk errors unrelated).
