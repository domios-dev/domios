---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: orphan_markdown
id: tasks/remote-worker-nesting-auto-link-and-cli-install-fix-for-1-5-7.md
handle: remote-worker-nesting-auto-link-and-cli-install-fix-for-1-5-7
title: tasks/remote-worker-nesting-auto-link-and-cli-install-fix-for-1-5-7.md
---
Investigate and fix why an existing remote pane assigned/treated as this orchestrator's worker does not automatically appear nested under the orchestrator. Expected behavior: remote workers spawned or assigned with report-to/parent metadata should create/update durable RemoteWorkerRegistry entries and sidebar nesting without manual local link. Also investigate why installed PATH CLI stayed 1.5.5 while app resources CLI was 1.5.6 after update, because missing --report-to made correct spawning impossible. Fix on a separate release branch, gate with routing/registry/UI tests, and prepare release 1.5.7. Do not touch prod 7685; use isolated dev instances.
