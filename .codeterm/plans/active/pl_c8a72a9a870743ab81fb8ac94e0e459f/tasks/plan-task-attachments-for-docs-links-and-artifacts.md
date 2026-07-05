---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: task
id: task_e2b51766d2dc4362a0b4177b8aaf1f7e
handle: plan-task-attachments-for-docs-links-and-artifacts
title: Plan task attachments for docs, links, and artifacts
status: done
tags:
- orchestration
- plan
- attachments
---

Design and implement first-class task attachments for CodeTerm plans.

Problem:
- Tasks currently carry text fields and lane metadata, but there is no clean way to attach related Markdown docs, external links, screenshots/artifacts, research notes, traces, or typed references.
- Orchestration needs durable evidence and context attached to the exact task instead of scattered chat messages.

Desired shape:
- Add a typed attachment model on tasks, with default kinds such as markdown, link, artifact/file, screenshot/image, trace, research, release, and custom.
- Attachments should have label/title, kind, target/path/url/body reference, optional description, created/updated metadata, and stable ids/handles.
- Support CLI add/list/remove/update for task attachments and generated docs/manifest if command surface changes.
- Support persistence in plan JSON/sidecars as appropriate; avoid bloating plan JSON for large markdown/artifacts.
- Render attachments in task detail/markdown export and eventually Canvas task cards/inspector in a compact way.
- Include validation for missing files/bad URLs where local validation is practical, but do not block remote links unnecessarily.

Acceptance:
- Storage model and CLI/API design are documented or self-evident through generated CLI docs.
- Focused tests cover add/list/remove, persistence reload, markdown export, and sidecar behavior if used.
- No automatic archiving or unrelated plan churn.
