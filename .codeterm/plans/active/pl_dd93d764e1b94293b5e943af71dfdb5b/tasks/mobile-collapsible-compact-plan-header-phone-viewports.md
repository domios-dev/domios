---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_b45b0a23561740208d703e6453752e6c
handle: mobile-collapsible-compact-plan-header-phone-viewports
title: 'Mobile: collapsible compact plan header (phone viewports)'
status: done
---

## What

On phone viewports the plan context block (state chip, peer chips, title, id/progress/sections, spec link, full goal paragraph with live focus, plan-skills registry) consumes ~60% of the screen before any tasks render (evidence: two HEIC screenshots, Board and List, in /Users/rollacode/.codeterm/uploads/). Make the plan header COMPACT and collapsible on narrow viewports; desktop unchanged.

## Design

1. **Compact bar (default on narrow).** On viewports under the md breakpoint (or coarse pointer), the header collapses to ONE ~48px bar: [state chip] Plan title (truncated at word boundary) · "99/104" progress · chevron. That is the full orientation minimum; everything else folds.
2. **Expand affordance.** The whole bar is a button (aria-expanded, 44px+ target). Tap -> inline accordion expansion of the full context (peer chips, goal, spec link, skills registry, focus line), scrollable within itself; tap again or the chevron collapses. No overlay sheet — inline keeps spatial continuity.
3. **Defaults & persistence.** Narrow viewport: collapsed by default. Desktop: current layout, untouched (breakpoint-gated, no behavior change above md). The expanded/collapsed choice persists in UI prefs per device-width class.
4. **Interplay.** The in-flight strip and status filters remain visible below the compact bar (they are the live signal and the working controls — they must not fold); the List filter row may compact to a single Filter chip on narrow as a secondary refinement (note in report if done).
5. **BONUS BUG (fix in this lane, same surface):** the skills registry renders a RAW error string ("The string did not match the expected pattern.") — route it through the shared quiet ErrorState from the resilience work; the skills section hides on error with a small retry, never prints exception text.

## Executable test plan (Playwright, mobile emulation 390x844)

1. Default: header height <= 56px on load; tasks visible above the fold (board first column top edge within upper half of viewport).
2. Tap bar -> expanded shows goal text + peer chips + skills; aria-expanded true; tap again collapses; state persists across reload.
3. Desktop viewport (1280x800): DOM/layout unchanged vs current (snapshot or key-element assertions — no compact bar rendered).
4. Seed a skills-registry error -> no raw exception text anywhere in the header (regex scan of the header container), quiet error state shown.
5. A11y: bar is a button, focus outline visible, chevron has aria-label.
