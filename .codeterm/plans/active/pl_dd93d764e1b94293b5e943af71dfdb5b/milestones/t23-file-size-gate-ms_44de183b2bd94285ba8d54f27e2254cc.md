---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_44de183b2bd94285ba8d54f27e2254cc
handle: t23-file-size-gate-ms_44de183b2bd94285ba8d54f27e2254cc
title: review
---

Bounced t23 file-size lane. Current diff would regress accepted chat image-output behavior by deleting ToolOutputContent/toolImageOutput; worker must rebase onto current feat/canvas-updates head and preserve image previews while keeping file-size gate green.
