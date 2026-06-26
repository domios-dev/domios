---
tags:
- orchestration
- mesh
- release
---

Build first-class support for workers running on a different mesh peer while still being attached to and reporting to the spawning orchestrator.

Problem:
- Today `codeterm agent report` works reliably for local child panes because the parent/target pane is on the same daemon.
- If an orchestrator delegates work to another machine through mesh, the remote worker can run, but it is not first-class attached to the local orchestrator. Reports/messages require manual `codeterm send --mesh ... --pane ...` routing, which is error-prone and blocks real distributed orchestration.

Desired behavior:
- An orchestrator on machine A can spawn or attach a worker on machine B.
- The worker has a clear parent/report target pointing back to the real orchestrator pane on machine A.
- `codeterm agent report` from that remote worker reaches the orchestrator automatically, just like a local worker report.
- Errors are explicit and actionable when no remote report target exists.
- The implementation must use existing send/report plumbing where possible; do not invent screenshot detection or plugin-side hacks.
- The release must be cut from the normal master/develop release path, not from `feat/canvas`.

Acceptance:
- CLI/API UX for remote worker spawning/reporting is documented in generated CLI/docs where relevant.
- Add unit/integration coverage for report target serialization/routing.
- Add a real mesh smoke or a well-isolated test seam proving remote report delivery semantics.
- Verify locally on the remote machine and then release a build/tag according to the repo release process.
- Report back to orchestrator pane 4d479b6c with exact branch, commits, tests, release tag/artifacts, and any remaining risks.
