---
tags:
- friction
---

F2 related-work + F3 ghosts both want to up-weight older/stalled tasks but PlanSearchHit (service/plan/search_index.rs) has no updated_at/age; fell back to status-only. Add the field to hit struct + recall response. Plan-BE lane.