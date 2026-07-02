---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: orphan_markdown
id: sections/v3-externalized-bodies-editable-md-files.md
handle: v3-externalized-bodies-editable-md-files
title: sections/v3-externalized-bodies-editable-md-files.md
---
Large section/task bodies move OUT of the plan JSON into sibling .md files (<plan_id>/sections/<id>.md, <plan_id>/tasks/<id>.md), wired by convention. Edit with any editor → the file-watcher reloads → board updates live. No CLI for text edits. JSON keeps structure only. Single-writer + atomic write preserved; migrate inline bodies on load.
