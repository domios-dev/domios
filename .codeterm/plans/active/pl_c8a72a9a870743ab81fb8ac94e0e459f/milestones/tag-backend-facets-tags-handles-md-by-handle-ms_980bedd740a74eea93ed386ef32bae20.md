---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_980bedd740a74eea93ed386ef32bae20
handle: tag-backend-facets-tags-handles-md-by-handle-ms_980bedd740a74eea93ed386ef32bae20
title: decision
---

Design decision: implement tags and handles as stored plan fields plus deterministic normalization before every write/read. Handles remain aliases (ids stay authoritative) and externalized body filenames derive from handle with id fallback for migration, so existing uuid .md files hydrate once and rewrite under readable handle names.
