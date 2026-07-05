---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_ef113535023d4c738d59a29a2dadc832
handle: plan-lineage-and-ghost-links-ms_ef113535023d4c738d59a29a2dadc832
title: decision
---

ANDREY: want plans that 'flow out of' each other — a DIRECTED/typed lineage link (derived_from / spawned_by), persisting even after the source plan is archived & off-canvas, so we REMEMBER there was a related plan and the connection. Current state: Plan.links exists (flat Vec<String>) + plan link/unlink CLI; survives archive (archive=status flag, not delete). GAPS: (1) link is undirected/untyped — no 'flows out of' lineage; (2) cross-plan links NOT rendered on canvas at all (only milestone bubble links are); (3) archived plans fully filtered from board — no ghost/memory marker for an archived related plan. BUILD: typed directed plan link (relation kind), canvas edge between related plans, and a faint GHOST node/marker for an archived linked plan (remember it existed + the link). Composes with handoff lineage (a /new fresh plan can be derived_from the old).
