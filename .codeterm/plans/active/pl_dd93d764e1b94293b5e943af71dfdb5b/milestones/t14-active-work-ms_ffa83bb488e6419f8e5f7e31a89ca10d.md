---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_ffa83bb488e6419f8e5f7e31a89ca10d
handle: t14-active-work-ms_ffa83bb488e6419f8e5f7e31a89ca10d
title: status_update
---

Accepted in c3d51153. Added shared active-work selector and surfaced in-flight work across PlanSwitcher, List, Canvas, Sidebar, Composer. Gates: activeWork, ActiveWorkStrip, ComposerPlanActivityStrip, paneComposerActivity, presenceOverlays, WorkspacePlanEntry, PlanListView tests passed; source diff-check and file-size gate passed; repo tsc only showed pre-existing PlanSwitcher.test never.kind errors; push hook cargo check/fmt/clippy passed.
