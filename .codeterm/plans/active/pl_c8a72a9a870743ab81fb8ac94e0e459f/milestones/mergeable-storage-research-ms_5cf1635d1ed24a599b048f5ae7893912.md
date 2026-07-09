---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_5cf1635d1ed24a599b048f5ae7893912
handle: mergeable-storage-research-ms_5cf1635d1ed24a599b048f5ae7893912
title: decision
---

Use git-native text files as authoritative storage: file-per-entity YAML/JSON frontmatter plus Markdown bodies for specs/plans/tasks/mem notes, and append-only event files for strides/status/history. SQLite/FTS/vector stores remain derived indexes rebuilt by plan/mem reindex.
