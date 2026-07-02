---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_934852491b8742bd9666b11d28b9a105
handle: workspace-moved-tabs-visible
title: Workspace moved tabs visible in sidebar
status: done
---

Fix the sidebar/workspace tab rendering path after workspace move-tab. Repro: move agent tabs from General to codeterm with workspace move-tab; API shows codeterm pane_ids/tab_count include them, but expanded workspace UI does not show those tabs. Manual tabs in the workspace render correctly. Find whether move-tab event/store normalization misses a tab list update, or the UI filters moved agent tabs incorrectly.
