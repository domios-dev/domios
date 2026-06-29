---
tags:
- friction
---

F2 'related work' + F3 'ghosts' both want to up-weight OLDER/stalled tasks, but PlanSearchHit (src-tauri/src/service/plan/search_index.rs) exposes no updated_at/age. Both lanes fell back to status-only filtering. Add updated_at (or age) to the hit struct + recall response so the FE can time-weight. Plan-BE lane.