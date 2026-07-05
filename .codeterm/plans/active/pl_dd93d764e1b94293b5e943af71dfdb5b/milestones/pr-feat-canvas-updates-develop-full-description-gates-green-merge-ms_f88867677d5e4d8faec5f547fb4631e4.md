---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_f88867677d5e4d8faec5f547fb4631e4
handle: pr-feat-canvas-updates-develop-full-description-gates-green-merge-ms_f88867677d5e4d8faec5f547fb4631e4
title: decision
---

ISSUE BURN-DOWN WAVE added to the release gate (round-47, user directive): ALL open GitHub issues must be resolved before release EXCEPT #175 (mem: unified memory — explicitly deferred, stays open). #192 (roadmap canvas-native watchers) closed outright per user. Remaining 14 issues mapped to 10 verify-first tasks (each: gh issue view N is the spec; reproduce on branch dev; already-fixed -> close issue immediately with evidence; else fix, reference bare #N so publish closes it): #209 Linux updater (RELEASE BLOCKER, coordinate with ubunta lane), #203 Windows conhost storm (ROG lane), #188 macOS WKWebView reload, #189 chat virtualization/perf (linked to existing longtasks lanes), #204+#199 codex TUI robustness, #205+#184 web auth, #207+#179 cron/hidden-run reliability (#207 submit-half may be fixed by wake-confirmer lane), #182+#180 pane lifecycle, #185 icons verify-first, #206 general-agent mem-skill prompt. This develop-PR task now ALSO gates on the burn-down wave.
