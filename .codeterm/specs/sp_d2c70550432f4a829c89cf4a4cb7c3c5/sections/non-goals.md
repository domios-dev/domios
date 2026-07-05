Non-goals for this effort:
- No backend/Rust planner model changes; this is a frontend UX effort over existing plan data (exception: none expected; if a worker believes a REST change is needed, report blocked and return to Planner).
- No drag-and-drop task status editing on Board (inspect-first; mutation UX is a future effort).
- No new Canvas features (edges rework, new node kinds, world decorations) — only legibility/perf/LOD.
- No theming system changes; use existing --ct-* tokens.
- No mobile/touch optimization.
- No removal of Canvas — it stays fully functional as tertiary view.
