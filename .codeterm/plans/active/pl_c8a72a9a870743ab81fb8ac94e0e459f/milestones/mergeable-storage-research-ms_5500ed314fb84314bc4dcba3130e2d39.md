---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_5500ed314fb84314bc4dcba3130e2d39
handle: mergeable-storage-research-ms_5500ed314fb84314bc4dcba3130e2d39
title: decision
---

MEM FACTS travel via the SAME model. Workspace-scoped facts = one .md per fact under .codeterm/mem/<area>/<id>.md (frontmatter id/kind/area/tags/source_path; body=fact). New facts=new files=no conflict; edits=LWW. file_sync.rs/source_path already does files->sqlite reindex so 'files are source, sqlite is cache' is half-built. GLOBAL/identity/peer facts stay home-sqlite NEVER git (matches scope split). Vectors derived.
