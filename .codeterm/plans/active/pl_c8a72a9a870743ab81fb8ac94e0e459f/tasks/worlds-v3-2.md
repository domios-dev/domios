---
tags:
- canvas
- worlds
- ux
---

# WORLDS v3 — plan-centric canvas composition (§35)

Re-open of worlds-v2. Andrey's live review of the board. Loop/pass on the worlds rendering.

## Direction (7 points)
1. **Plan-centric composition.** The PLAN world is the central, LARGEST object — bigger than all else, occupying the center. LEFT = the SPEC it was built from. RIGHT = the TASKS. Size hierarchy: plan > spec ≈ tasks; a task can NEVER exceed plan/spec size.
2. **Task cloud = circle pack, NOT a line.** Tasks today form a line (was vertical, now horizontal — both wrong). Want cubic task blocks INSCRIBED in a CIRCLE, non-overlapping, round cloud right of the plan. Needs a concrete packing algorithm (phyllotaxis / circle-pack relaxation / concentric rings) — stable, no jitter on add/remove (§31).
3. **Task zoom-in, STABLE layout.** Fixed slots that never reflow: top=TITLE, center=DESCRIPTION, bottom=WORK-POINTS row. All three visible even far; only CONTENT detail changes per LOD band (far: title+tags; closer: short desc; closest: full markdown, scroll only at the closest band). Blocks stay put, content swaps inside.
4. **Work-points (strides).** Circles are RIGHT. Zoom into one → tags → text → full structure. BUG: text renders AROUND the point, must be INSCRIBED inside. Deepest band = work-point fills the screen for reading. No flying — fixed slot at every zoom.
5. **Sections show FULL content, not truncated.** Progressive reveal: far=title+tags; closer=body; closest=rendered markdown paragraphs.
6. **Spec vs plan correctness.** Plan currently REPEATS spec titles and is UNDER-FILLED (sections have no names — confirmed). Plan must be filled DISTINCT from + more detailed than spec (plan=HOW/tasks/strides, spec=WHAT). Fill the plan-system from the orchestrator's working markdown.
7. **Emergence / loops.** Whole feature supports PASSES/LOOPS: every object (spec/plan/section/task/stride) re-enterable and loopable. Data model + canvas must express "pass N / loop" (lineage/iteration count, re-open semantics, visual signal). Ties to `codeterm plan loop`.

## Process
Brainstorm (opus+codex) parses these words vs the code, proposes §35 design. Then implement as worlds-v3 (workers write code+tests, orchestrator is the single build gate). Review live on the board before trusting.
