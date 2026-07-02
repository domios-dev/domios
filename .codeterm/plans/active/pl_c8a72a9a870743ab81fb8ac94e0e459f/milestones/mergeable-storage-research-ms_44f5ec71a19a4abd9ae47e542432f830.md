---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_44f5ec71a19a4abd9ae47e542432f830
handle: mergeable-storage-research-ms_44f5ec71a19a4abd9ae47e542432f830
title: decision
---

MERGE MECHANISM = git-native 3-way per-file + built-in 'merge=union' for *.jsonl logs. NO custom semantic merge driver, NO CRDT, NO Dolt. .gitattributes: '.codeterm/**/*.jsonl merge=union'. .gitignore: .codeterm/**/index.sqlite*, vectors, cache/. The whole point: the layout makes git's DEFAULT merge correct, so the diff stays human-readable and PR-reviewable. Custom driver would only be needed if we kept the monolith (we don't).
