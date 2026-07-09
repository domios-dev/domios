---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_e13e91757e264a5fbc732627adaa20fa
handle: branch-review-r5-sidebar-shared-terminal-chatview-lib-state-diff-vs-deve
title: 'Branch review R5: Sidebar, shared, Terminal, ChatView, lib state (diff vs develop)'
status: done
---

Review git diff origin/develop...HEAD (merge-base 40ba275ffc43) RESTRICTED to src/components/Sidebar/, src/components/shared/, src/components/Terminal/, src/components/ChatView/, src/lib/. KNOWN BUG CLASSES shipped from these areas — check the branch still doesn't contain them or cousins: (1) reconnect banner flapping ~500ms over healthy API (hysteresis missing — a stabilization task exists, verify current code state), (2) ghost tabs restored for dead panes, (3) sidebar re-sort flicker (sortStateRef reset on remount; FLIP animating on any top delta), (4) chat status via verb-list matching instead of structural fingerprints. Hunt CRITICAL/HIGH: panes-list/tab desync, subscription leaks, sub-1s polling loops, unstable React keys causing remount storms, xterm lifecycle (double-open, missed dispose, fit on zero-height), crashes on undefined pane/agent fields during session restore. OUTPUT CONTRACT: strides + detailed fix tasks for cheap models, same as R4.
