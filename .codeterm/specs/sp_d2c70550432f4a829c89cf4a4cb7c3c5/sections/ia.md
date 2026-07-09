View priority and mental model (user-mandated):
1. Board — primary, DEFAULT view when a plan opens. Kanban of task status with plan/spec context always reachable.
2. List — secondary. Dense hierarchical read of spec -> plan -> sections -> tasks for orientation and audit.
3. Canvas — tertiary. Spatial/presence overview for orchestration theater; never the default, never required for daily work.

Requirements:
- The mode switch renders in Board, List, Canvas order; Board is the default (`DEFAULT_PLAN_VIEW_MODE`), replacing today's canvas-first order in src/components/Canvas/planViewMode.ts.
- Selected view mode persists across plan open/close and app restart (per-user setting; per-plan override optional, not required).
- All three views share one data source (PlansProvider) and one task/plan/spec detail surface; a task is the same object no matter which view opened it.
- Every view answers within 5 seconds of looking: what is this plan trying to do (goal), what is being worked now, who works on it, what is blocked, what remains.
