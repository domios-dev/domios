---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_1f551fc5c5f64088a6021a0a7ea3a878
handle: mergeable-storage-research-ms_1f551fc5c5f64088a6021a0a7ea3a878
title: decision
---

MERGE-STORAGE synth (codex 0e4c39b2, 1/2). RECO = HYBRID git-native: authoritative TEXT files per entity + APPEND-ONLY event files for strides/status; sqlite/FTS/vectors always DERIVED, never in git. Layout: .codeterm/plans/<id>/meta.yaml + sections/<handle>/section.md (frontmatter+body) + tasks/<handle>/task.md + tasks/<handle>/events/<ulid>-<actor>.json (strides/status/assign/edit). specs same. mem: .codeterm/mem/items/<kind>/<id>.md + events. MERGE: ordinary git per-file (different entities = no conflict; same body edit = normal text conflict = correct human conflict); strides/status = append-only one-file-per-event → merge = file UNION, NO custom driver. Fold = snapshot + events by idempotent set-union + deterministic order. Scalar fields = Lamport tuple (logical_clock, actor, event_id) LWW ONLY where genuinely scalar; preserve competing body/text edits as git conflicts (NOT silent LWW). Single-writer per-worktree daemon; git = multi-writer transport; on merge/pull watcher folds + reindex (indexes discarded/rebuilt). Migration = one-time JSON→file-per-entity. Alts: pure-snapshot (status still conflicts), full-event-sourcing (every read needs fold), CRDT/Automerge (auto-merge but binary dep + parity work), Dolt (SQL+merge but heavy). Why not current big JSON: one large conflict surface (task arrays/milestones/spec_links/canvas collide).
