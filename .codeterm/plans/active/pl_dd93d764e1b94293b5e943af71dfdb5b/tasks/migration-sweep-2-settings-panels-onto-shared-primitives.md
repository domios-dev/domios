---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_bfc0094519fc45618643856fdbe4a429
handle: migration-sweep-2-settings-panels-onto-shared-primitives
title: 'Migration sweep 2: Settings panels onto shared primitives'
status: done
---

Migrate Settings surfaces onto the shared design primitives introduced by task_e9a1038b. Scope is Settings panels only: src/components/Settings/** plus directly-owned shared controls used only by those panels if needed. Use Button, Chip, Card, and FormRow instead of bespoke button/chip/card/form-row markup where the audit inventory marks duplicate usage. Do not touch Mission Control, Canvas plan surfaces, or sidebar group rendering. Keep behavior identical; this is a primitive adoption and visual consistency sweep, not a settings feature rewrite. Update shared.grepgate baselines only downward for the affected counts. Attach or capture before/after screenshots for the main Settings tabs changed.
