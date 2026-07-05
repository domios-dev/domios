---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_b44774c8c080412581ab32f787585288
handle: mergeable-storage-research-ms_b44774c8c080412581ab32f787585288
title: decision
---

Merge mechanism should be normal Git plus deterministic generated files: one file per entity limits conflicts; append-only event files merge by set union sorted by ULID/actor/clock; generated INDEX.md and SQLite/vector indexes are not hand-merged.
