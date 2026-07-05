---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_8a1413e715934bc3b70171b91ad82376
handle: plan-store-coherence-watch-the-sidecar-dir-reconcile-external-changes
title: 'Plan store coherence: watch the sidecar dir, reconcile external changes'
status: done
---

Proven gap (round-22): plans were deleted through the PROD daemon (sidecar files removed from disk) while the DEV daemon kept serving all of them from memory — the UI showed ghosts until a manual restart. Any external change has the same blindness: the OTHER daemon's deletes, git checkout/pull/rebase rewriting .codeterm/plans (lanes do this constantly), manual file edits. Puccini's bcf638f8 fixed BOOT staleness (503 while scanning + plans-loaded event + immediate refetch); this task fixes RUNTIME staleness. FIX: the daemon watches the project-local plans directory (same FS-watch machinery the Librarian uses for skills) and reconciles its in-memory store on external changes — evict deleted plans, reload changed plan.json/sidecars (respecting its own in-flight writes: ignore events for paths it just wrote, standard self-echo suppression via a recent-writes set), then emits plans-changed over the bridge; the frontend already refetches on that family of events after bcf638f8. Debounce bursts (git checkout touches many files — coalesce to one reconcile per ~500ms). Executable test plan: (1) integration on dev: create a scratch plan via CLI against PROD daemon, assert it appears in the DEV daemon's /api/plans within 2s WITHOUT restart; delete it via prod, assert eviction within 2s; (2) git-burst test: touch 20 sidecar files in one commit checkout, assert exactly one reconcile pass in the log (debounce proof); (3) self-echo test: a plan write through the daemon itself does NOT trigger a reconcile loop (log assert); (4) unit: recent-writes suppression set semantics.
