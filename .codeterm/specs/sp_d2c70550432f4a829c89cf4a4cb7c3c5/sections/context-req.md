Plan and spec context (goal vector, focus, spec lead, owner, progress) must be visible and inspectable from every view without awkwardness.

Requirements:
- Board/List: a shared compact plan-context header band (one component used by both) showing title, goal vector (truncated with expand), heartbeat focus, owner, done/total; clicking expands to full plan/spec via the shared detail surface.
- The header is calm: no more than two lines tall collapsed, consistent typography with the rest of the app (var(--ct-*) tokens).
- Spec reachable in ≤2 interactions from any view (header -> spec detail).
