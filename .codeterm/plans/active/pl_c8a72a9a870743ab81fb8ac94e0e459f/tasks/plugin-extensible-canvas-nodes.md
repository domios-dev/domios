---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_5fb8c33696ef4277bf6ee0d235fa51bc
handle: plugin-extensible-canvas-nodes
title: Plugin-extensible canvas nodes (external e.g. GitHub) + agent CLI to read them
status: done
---

Foundation slice for plugin-extensible canvas nodes: implement agent-facing read access for existing canvas elements (Plan.elements) so agents can query external/open canvas nodes. Keep scope to CLI/socket/API list/show/read helpers and tests; do not build full plugin renderer registration or broad host write APIs in this slice unless an existing safe primitive only needs wiring.
