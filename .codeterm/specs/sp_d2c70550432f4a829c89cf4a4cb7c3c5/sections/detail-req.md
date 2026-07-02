One shared detail surface for task/plan/spec inspection, reused by Board, List, and Canvas.

Today CanvasDetailOverlay + canvasRender/DetailContent.tsx implement a good fullscreen READ surface, but the API is canvas-coupled: it needs LaidOutNode, nodeById map, and resolveModel. Board and List have nothing.

Requirements:
- Extract a view-agnostic detail overlay (working name PlanDetailOverlay) whose props are model-level: {kind: task|plan|spec, task?, plan?, spec?, panes, onOpenPane, onClose}. Canvas keeps its wrapper that adapts LaidOutNode to this API; Board and List call it directly.
- Task detail shows: name, handle, status + status history freshness, body, acceptance criteria, test plan, correctness, section links, lane (branch/worktree/workspace), executor presence + history, milestones/strides timeline, and an "open pane" action when an executor pane exists.
- Plan detail shows goal (vector/vision/constraints/postulates), heartbeat focus, progress; spec detail shows section tree.
- Overlay behavior: fullscreen over the view, Esc closes, focus-trapped, scrollable, closing restores the underlying view untouched (no camera moves, no scroll loss).
- One source of truth: no duplicated task-rendering logic between views; DetailContent stays the single body renderer.
