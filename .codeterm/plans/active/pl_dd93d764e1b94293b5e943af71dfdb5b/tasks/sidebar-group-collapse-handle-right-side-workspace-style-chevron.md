---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_2dccf6ef329e4e1fb4c9123e38adae38
handle: sidebar-group-collapse-handle-right-side-workspace-style-chevron
title: 'Sidebar group collapse handle: right-side workspace-style chevron'
status: done
---

Round-43 refinement from Andrey: orchestration groups must collapse like workspaces. The collapse handle belongs on the RIGHT side of the orchestrator group head row, visually matching the WorkspaceItem chevron family, not as a small left-side chevron before the provider glyph.

Scope:
- Reposition/restyle the existing group collapse toggle; do not redesign containment.
- Default remains expanded.
- Collapsed head keeps the summary line, e.g. "1 manager · 2 workers".
- Children, section headers, and the containment hairline disappear as one unit when collapsed.
- Keep data-testid="group-collapse-toggle" so existing tests survive.
- Match WorkspaceItem chevron feel: muted, hover brightens, right-aligned, rotation animation around 320ms cubic-bezier.
