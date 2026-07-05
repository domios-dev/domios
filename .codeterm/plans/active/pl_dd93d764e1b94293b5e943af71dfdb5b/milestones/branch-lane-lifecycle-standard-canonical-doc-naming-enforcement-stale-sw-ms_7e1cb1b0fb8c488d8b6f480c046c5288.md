---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_7e1cb1b0fb8c488d8b6f480c046c5288
handle: branch-lane-lifecycle-standard-canonical-doc-naming-enforcement-stale-sw-ms_7e1cb1b0fb8c488d8b6f480c046c5288
title: decision
---

SCOPE ADDITION — DELIVERABLE 5 (round-74, Orchestrator incident: 'git push origin --delete <branch>' fired the pre-push FULL hook and launched cargo check during cleanup — interrupted by hand). PRINCIPLE for the standard doc: CLEANUP OPERATIONS ARE GATE-FREE — deleting a branch/worktree verifies nothing new; heavy gates guard content INTEGRATION, not content REMOVAL. MECHANICAL FIX (this repo's .githooks/pre-push, ~5 lines): the hook reads ref updates from stdin — when EVERY pushed ref is a deletion (local sha is the zero-sha), exit 0 before any check; mixed pushes (deletion + real ref) still gate the real refs. Do NOT document --no-verify as the cleanup path — training agents to bypass hooks resurrects the class this whole wave kills; the hook must simply be correct about what a deletion is. INHERITANCE: the auto-cleanup lane (task_540e7afa) performs programmatic branch deletions — its acceptance gains a drill: branch delete via the daemon triggers zero gate processes (ps-scan proof during the drill). Lifecycle doc line: 'Deleting branches and removing worktrees never runs gates; if a deletion asks you to compile something, the hook is wrong — fix the hook, do not bypass it.'
