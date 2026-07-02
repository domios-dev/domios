The spec ENTITY itself renders badly today (user evidence: canvas screenshot of the spec card). Problems: section titles truncate into noise ("UX problem stateme…"), every row carries an identical "→ tasks" chip that says nothing, and the card is mostly dead space; the spec is a wall of cropped strings instead of a readable requirements summary.

Requirements:
- Canvas spec card: compact summary — spec title, section count, coverage rollup (n covered / n unplanned), and at most the top-level section titles with enough width to read; no per-row identical chips. Rows show a meaningful coverage state (covered / partial / unplanned glyph + task count) only when informative.
- Clicking the spec card (or its View) opens the shared detail surface with the FULL section tree: untruncated titles, section bodies on expand, per-section linked plan sections and task coverage.
- Board/List: spec is reachable via the plan-context header (spec title + section count), opening the same spec detail; no separate diverging spec renderers.
- Same readability rule as everything else: no truncation that destroys meaning; truncate with expand affordance, or wrap.
