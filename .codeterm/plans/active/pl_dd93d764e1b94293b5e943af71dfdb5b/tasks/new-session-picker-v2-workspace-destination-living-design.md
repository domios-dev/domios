---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_234c14f04d9f4dbfb5a743d50486617d
handle: new-session-picker-v2-workspace-destination-living-design
title: 'New-session picker v2: workspace destination + living design'
status: done
---

## What

New-session picker v2 (screenshot round-14): add workspace context and bring the design to life. Current card is a flat gray form with no sense of place — the user cannot tell WHERE the session will land, and nothing about it feels crafted.

## UX additions

1. **Workspace row.** A workspace select, preselected to the CURRENT workspace, listing all workspaces (+ "New workspace…" option that inline-creates one by name). Phrased as destination, not a bare field: "Creates in: <workspace>".
2. **Instant explainer.** A small (?) glyph next to the workspace label; hover shows an INSTANT tooltip (0ms delay, not the default hover wait): "A workspace groups related tabs — usually one project or effort. Your new session opens as a tab inside it." Same tooltip pattern reusable for the mode toggle (what an Orchestration pair is).
3. **Destination preview line.** Under the form, one live line that always states the outcome: "Claude Code (Opus 4.8) will open in <workspace>" / "Orchestrator + Planner will open as one group in <workspace>" — updates with every selection. This is the what-will-happen answer in words, always visible before commit.

## Design energy (quiet-bold, CodeTerm palette — no generic form)

- **Mode choice as two rich cards**, not a text segment: "Single agent" (one glyph tile) vs "Orchestration" (two glyph tiles joined by the pair bracket). Selected card gets an accent ring + slight lift; unselected stays muted.
- **Pair assembly moment:** when Orchestration is selected, the two provider rows connect with a short draw-in of the pair bracket (~250ms, reduced-motion: static) — the pairing IS the product moment, let it land.
- **Provider rows as glyph tiles** (provider icon + name) instead of native selects where cheap; model select stays a quiet dropdown.
- **Ambient wash:** whisper-quiet radial tint behind the active section in the accent/presence palette (alpha <=0.06), consistent with the liveliness grammar.
- **Staggered reveal** on card open (~120ms cascade, CSS only).
- CTA: accent-filled, reflects mode ("Start session" / "Start orchestration").

## Coordination

Same surface as task_e0ddee081c15441281e169b3ae3c71a3 (Orchestration group v2 — in-place spawn): SEQUENCE these or run as one lane; the workspace select feeds the in-place logic (creating into a DIFFERENT workspace cannot transform the current tab — it opens the group in the target workspace and says so in the preview line).

## Playwright spec — write EXACTLY these scenarios (web-mode, tests/playwright/suites/20-new-session/picker.spec.ts)

Preconditions: dev healthy on 7686; run: `npx playwright test --config=tests/playwright/playwright.config.ts --project=golden -g "new-session"`. Required testids (add in implementation): new-session-picker, picker-mode-single, picker-mode-orchestration, pair-affordance, picker-workspace, workspace-help, workspace-help-tooltip, picker-preview, picker-confirm, picker-provider-orchestrator, picker-model-orchestrator, picker-provider-planner, picker-model-planner.

1. OPEN: click sidebar New session in workspace A -> a tab opens whose body has [data-testid=new-session-picker]. PASS: picker visible, no agent spawned yet (pane list unchanged except the empty tab).
2. MODE CARDS: both mode cards visible; click picker-mode-orchestration -> it gets data-selected=true, pair-affordance becomes visible. PASS: exactly one card selected at a time.
3. DEFAULTS: with no prior persistence, orchestrator provider=claude/model contains "Opus", planner provider=claude/model contains "Fable". PASS: select values assert exactly.
4. WORKSPACE ROW: picker-workspace value equals current workspace name. Hover workspace-help -> workspace-help-tooltip visible within 100ms (instant, no hover delay). PASS: tooltip text mentions "workspace".
5. PREVIEW LINE: picker-preview text contains "Orchestrator + Planner" and workspace A name; select workspace B in picker-workspace -> preview updates to B without reload. PASS: exact text assertions before/after.
6. PERSISTENCE: change planner model to a non-default; close the picker tab; open New session again -> mode=orchestration and the changed model are preselected, workspace is back to CURRENT (workspace choice must NOT persist). PASS: all three assertions.
7. IN-PLACE PAIR SPAWN (same workspace): note the picker tab id; click picker-confirm -> THAT tab now hosts the orchestrator pane (tab id unchanged), sidebar shows the orchestration group with planner as first child; total new tabs created = 1 (the planner). PASS: tab id equality + group structure + tab count delta.
8. GREETING: in the orchestrator chat, the first assistant message contains the PEER_ORCH_GREETING marker text ("Tell me the goal" fragment). PASS: message visible without user input.
9. CROSS-WORKSPACE: repeat spawn with workspace B selected -> group appears under workspace B in the sidebar; picker tab in A does NOT transform. PASS: group location + tab A state.
10. GROUP CLOSE: hover planner child row -> no close affordance rendered; click group head close -> confirm dialog lists orchestrator + planner; confirm -> both panes gone. PASS: affordance absence + dialog contents + post-close pane list.

Each scenario is independent (fresh picker per test via beforeEach); scenarios 7-10 clean up spawned panes in afterEach so reruns are stable.
