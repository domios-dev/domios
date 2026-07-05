# Canvas v2 — Overnight Iteration Loop (the build/review/polish machine)

<!-- revision: 1 -->

How we drive the Canvas v2 board from "structurally correct" to "genuinely
Figma-grade" — autonomously, overnight, ≥20–25 iterations, on real CodeTerm
subworkers, every cut design-reviewed with the `frontend-design` skill. The
orchestrator runs as **engineering + product manager with taste**: sets clear
personalities, feeds sharp critique, accepts no patches/laziness/legacy.

Design source of truth: `2026-06-24-canvas-v2-design.md`. This doc is the *process*.

## Roles & personalities (workers get these verbatim at spawn)

- **Mira — Lead Canvas Designer-Engineer** (`claude` opus + `frontend-design` skill).
  *You have impeccable visual taste — years on Figma-grade tools. You are physically
  unable to ship a board where text overflows a node, edges miss their anchor, or
  spacing is arbitrary. You think in hierarchy, rhythm, restraint. You refactor toward
  the right abstraction; you never bolt on a patch. You implement ONE focused
  improvement per iteration, beautifully, with tests, then commit+push.*
- **Dax — Adversarial Design Critic** (`claude` opus + `frontend-design` skill).
  *You are a ruthless, specific design critic. Flattery is failure. You score the
  screenshots against the rubric, name every defect with its node + pixel symptom +
  the concrete fix, and rank them. You compare against Figma/Linear/Excalidraw quality
  bars. "Looks fine" is never an acceptable review.*
- **Rex — Mechanical Fixer** (`codex` or `cursor`). *Fast, precise, bounded. You apply
  a specific list of fixes exactly, no scope creep, run the gates, commit+push.* Used
  for cheap mechanical defect batches so opus time goes to design judgment.

The orchestrator (Andrey's EM/PM proxy) owns triage, integration, the Playwright
screenshot/assert pass, and deciding each iteration's focus.

## The iteration cycle (one pass)

1. **Build** — Mira applies the top item(s) from the defect backlog (or, iter 0, the
   structural v2 base), commits + pushes to `feat/canvas-redesign`. Mechanical batches
   may go to Rex in parallel on a separate file set.
2. **Integrate** — orchestrator merges to `feat/canvas`, hot-reloads dev (no full
   restart when avoidable), confirms `build` hash + tsc/tsx/cargo gates green.
3. **Capture** — orchestrator runs the Playwright canvas harness: full-board screenshot
   + per-zone screenshots (plan node, spec node, task cluster, step bubbles), driving
   pan/zoom; plus structural asserts (§ checklist).
4. **Review** — Dax scores the screenshots with the rubric → numeric score + ranked,
   concrete defect list. (Early iterations may self-review inline to save spawns; from
   the midpoint on, spawn a fresh Dax so the eyes stay adversarial.)
5. **Triage** — orchestrator updates `canvas-iteration-log.md`, picks next focus,
   feeds critique to Mira/Rex.
6. **Loop** — repeat. Mid-loop, periodically **dogfood realtime**: add a section/task/
   step via the API and confirm it slots in without overlap/overflow; and **verify
   isolation** (the plan must NOT appear in another workspace's canvas).

## Design rubric (Dax scores each 0–10; weighted total /100)

| Dim | What "10" means |
|-----|-----------------|
| Hierarchy (×1.5) | One compact sectioned plan node + spec node + satellite tasks + step bubbles; instantly legible structure |
| Connection (×1.5) | Curved edges connect task→section row & task→steps; spec↔plan; nothing floats |
| Coverage (×1.0) | Covered vs uncovered section rows obvious via fill tint; uncovered pop |
| Bounds (×2.0) | NOTHING overflows/clips its node; text wraps; big sections confined to spec node (scroll) |
| Packing (×1.5) | Deterministic, zero overlap, fills space well; realtime add slots in cleanly |
| Typography (×1.0) | Consistent rhythm, readable sizes, good density, ellipsis where needed |
| Color/theme (×1.0) | ct theme vars only, restrained palette, accessible contrast |
| Navigation (×1.0) | Trackpad two-finger pan + cmd/pinch zoom smooth |
| Markdown (×0.5) | Renders in every node; no raw markup leaking |
| Polish (×1.0) | Alignment, micro-interactions, the intangible "Figma-grade" feel |

## Per-iteration Playwright checklist (structural, automatable)
- [ ] Full board + zone screenshots saved (timestamped).
- [ ] No node bbox overflows its content container; no text clipped.
- [ ] No two node bboxes overlap (beyond intended grouping).
- [ ] Every covered section row carries the coverage tint class.
- [ ] Edge count > 0 and endpoints resolve to real node ids.
- [ ] No raw markdown tokens visible in rendered nodes.
- [ ] Realtime: API-add a node → reflow has no overlap/overflow.
- [ ] Isolation: querying another project's canvas does NOT list this plan.

## Self-pacing (overnight, context-resilient)
- The orchestrator drives via a **self-heartbeat** (codeterm delayed self-message): each
  wake → check the active builder → if its cut landed, run integrate→capture→review→
  triage→spawn-next; else reschedule. Tuned so it never idles long nor spams.
- **`canvas-iteration-log.md`** (sibling file) is the durable loop state: one block per
  iteration (commit, score, top defects, next focus). On context compaction the
  orchestrator re-reads it and resumes — no history needed in context.
- Builder kept warm across iterations (critique sent each round); `codeterm agent
  compact` when context bloats; respawn fresh if needed. Critic respawned for fresh eyes.

## Stop criteria
- Score ≥ 90/100 on two consecutive iterations, OR 25 iterations reached, OR user halts.
- On stop: final integrate + push, restart dev clean, write a summary block in the log,
  Telegram the user only if EXTREME (per standing rule), else leave the board ready.

## Anti-patterns (rejected on sight)
Patches over refactors; magic numbers (use `settings.ts`); a node that scrolls the page;
text overflowing a box; "good enough" reviews; legacy left lying around; silent scope
caps (log what was deferred). Mira/Dax are briefed to refuse these.
