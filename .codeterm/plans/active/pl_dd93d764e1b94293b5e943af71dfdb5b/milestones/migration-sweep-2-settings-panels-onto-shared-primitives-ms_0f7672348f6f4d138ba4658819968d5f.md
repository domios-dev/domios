---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0f7672348f6f4d138ba4658819968d5f
handle: migration-sweep-2-settings-panels-onto-shared-primitives-ms_0f7672348f6f4d138ba4658819968d5f
title: status_update
---

Accepted and merged in 3d652be8 (Settings primitive migration). Changes: Settings Agents, Logs, Provider install, SchemaRenderer, and shared Settings helpers now use shared Button/Chip/Card/FormRow primitives where the audit marked duplicate usage. Integration fixes: Card/FormRow now forward standard div props so Settings data selectors stay stable; Settings Playwright keybinding smoke now matches the rendered keybinding surface and Tab glyph. Gates: npx tsx shared.grepgate passed with bespoke chip 22->16 and inline button still 16; SchemaRenderer.test, sections.test, Card.test, FormRow.test all passed; npx tsc --noEmit passed; Settings smoke Playwright project 3/3 passed with --timeout=30000 --global-timeout=600000; pnpm build passed; git diff --check, file-size gate, comment-block gate passed; pre-push shell guards, cargo check, fmt, clippy passed.
