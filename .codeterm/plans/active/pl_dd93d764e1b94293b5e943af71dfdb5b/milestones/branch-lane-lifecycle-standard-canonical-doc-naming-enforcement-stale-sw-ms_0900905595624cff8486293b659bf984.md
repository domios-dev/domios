---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_0900905595624cff8486293b659bf984
handle: branch-lane-lifecycle-standard-canonical-doc-naming-enforcement-stale-sw-ms_0900905595624cff8486293b659bf984
title: note
---

LOAD INCIDENT ADDENDUM (round-84, live: Mac-Air crawling under 5 editing agents): two stacked causes confirmed — (1) prod CodeTerm 1.6.2 runs WITHOUT every perf fix shipped on the branch (chat virtualization #189, memoized surfaces, longtask fixes): daemon 140% CPU + WebContent 1.3GB/130% under 5 streaming agents is exactly the fixed bug class, cure = the release itself; (2) the pre-push FULL gate fired a whole-crate cargo check locally (rustc 1.1GB RSS) because integration-branch pushes run the full gate BY DESIGN — on a weak machine during endgame this stacks onto agent load. Lifecycle-doc line to add: on constrained machines, integration pushes use PREPUSH_LIGHT=1 with heavy verification routed through the QA/builder gate (which the endgame flow does anyway), or push integration merges from a builder machine.