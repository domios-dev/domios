---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_f23c24b7d7e447a9ad83384d5a82685d
handle: mergeable-storage-research-ms_f23c24b7d7e447a9ad83384d5a82685d
title: decision
---

ADOPT hybrid file-per-entity + append-only event log (git-bug-shaped). Files are source of truth, sqlite/vectors always derived. Extend §17 from bodies to the WHOLE structural record; kill the monolithic plan.json arrays (tasks[]/sections[]/milestones[]). One entity = one file, identity in filename (uuid/ULID), never two entities in one mutable array in one file -> git merges file-by-file, conflicts only on genuine same-entity same-field edits.
