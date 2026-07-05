Board is the primary work surface. Current implementation (src/components/Canvas/planView/PlanBoardView.tsx, boardModel.ts, BoardTaskCard.tsx, styles in src/styles/globals.css .plan-board-*) fails in four ways visible in the evidence screenshots: cards are not clickable (no way to inspect a task), plan/spec context is two small summary cards visually disconnected from the columns, the Done column renders every done task (a 200-done plan becomes an infinite wall), and mostly-empty columns waste the viewport.

Requirements:
- Clicking a task card opens the shared detail surface (see task-detail section) without leaving Board; Esc/close returns to the exact board state.
- Plan context (goal vector, focus, owner, done/total progress, spec title) lives in a compact, well-designed header band above the columns; expandable to full plan/spec detail via the same shared detail surface. Not two floating cards.
- Done column is bounded: collapsed by default to the most recent N (~5) with a count badge and an explicit "show all" affordance that scrolls INSIDE the column, never growing the page. Column bodies scroll independently; column headers stay visible.
- Columns communicate emptiness cheaply (subtle placeholder), and the board fills available width without dead gutters.
- Cards keep current facts (handle, status, executor presence, lane branch/worktree, latest milestone) but with clear typographic hierarchy: title first, status compact, metadata subdued.
- Multi-plan workspace mode stacks per-plan boards with clear plan separation (current stack layout retained but restyled).
- Keyboard: arrow/tab navigation across cards, Enter opens detail (nice-to-have, not a gate).
