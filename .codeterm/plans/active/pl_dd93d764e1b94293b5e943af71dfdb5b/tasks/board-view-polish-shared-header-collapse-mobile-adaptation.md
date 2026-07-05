---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_4b7568d636264b2b82d959233a03388f
handle: board-view-polish-shared-header-collapse-mobile-adaptation
title: 'Board view polish: shared header collapse + mobile adaptation'
status: pending
---

Companion to the List polish task (shared PlanContextHeader — same collapse + honest-badge fixes apply automatically; do NOT duplicate them here, verify they hold on Board). BOARD-SPECIFIC SCOPE: (1) verify the dead-space layout drift does not affect Board (same shell) — fix if present; (2) MOBILE PASS at 390x844: columns must be usable on phone — horizontal swipe/scroll between status columns with snap points (or a stacked single-column mode if simpler), card touch targets >=44px, column headers sticky, done-column collapse still works; (3) card chips/text truncation on narrow widths (same smarter-truncation rule as List rows); (4) view-switch (Board<->List<->Canvas) on mobile keeps scroll position sane per view. Acceptance: phone-viewport screenshots of Board before/after + a swipe-through of all columns.
