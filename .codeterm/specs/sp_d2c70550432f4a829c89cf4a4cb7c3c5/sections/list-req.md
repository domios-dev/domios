List is the secondary, orientation/audit view. Current implementation (src/components/Canvas/planView/PlanListView.tsx, buildPlanListTree.ts) is a passive flat render: rows are not clickable, nothing collapses, done tasks sit inline at full weight, and long plans become one undifferentiated scroll (evidence screenshots 549be059, a39267bb).

Requirements:
- Rows are interactive: clicking a task row opens the shared detail surface; clicking a section row toggles collapse of its subtree; spec-root/plan-root rows open plan/spec detail.
- Sections are collapsible with persisted expand state while the view lives; default: sections with only done tasks start collapsed.
- Done tasks render visually quiet (dimmed title, compact row) and a fully-done section shows a "n done" rollup instead of n rows until expanded.
- A thin toolbar offers status filter chips (all / active / done / blocked) and a plain-text search that filters rows.
- Sticky headers: the current plan (and section, if feasible) header pins while scrolling its subtree.
- Keep the single-scroll dense character — List must stay the fastest way to read the whole effort top-to-bottom.
