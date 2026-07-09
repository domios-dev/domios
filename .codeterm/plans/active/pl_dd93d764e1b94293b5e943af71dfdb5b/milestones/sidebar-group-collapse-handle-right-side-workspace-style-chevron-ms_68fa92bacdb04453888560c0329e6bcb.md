---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_68fa92bacdb04453888560c0329e6bcb
handle: sidebar-group-collapse-handle-right-side-workspace-style-chevron-ms_68fa92bacdb04453888560c0329e6bcb
title: status_update
---

Accepted and merged in fc5e6c3e.

Changes:
- Orchestration group collapse handle moved from the left of the provider glyph to the right edge of the group head row.
- Toggle remains data-testid=group-collapse-toggle and is now the last interactive button in the head row.
- Collapse/expand rotation uses the requested 320ms cubic-bezier treatment.
- Integration fix: removed global width/height override so touch-mode hitbox sizing remains controlled by component classes.

Gates:
- npx tsx src/components/Sidebar/GroupContainment.test.tsx: 4/4 pass
- npx tsx src/components/Sidebar/TabItem.test.ts: 1/1 pass
- npx tsx src/components/Sidebar/OrchestrationGroupSection.test.tsx: 4/4 pass
- npx tsc --noEmit: pass
- pnpm build: pass
- git diff --check excluding .codeterm, file-size gate, comment-block gate: pass
- live DOM smoke: group-collapse-toggle count=1, lastIsToggle=true, data-collapsed=false; screenshot /tmp/sidebar-group-collapse-handle.png
- pre-push shell guards, cargo check, fmt, clippy: pass
