---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_829964912eb84b67b787b425494b843f
handle: mergeable-storage-research-ms_829964912eb84b67b787b425494b843f
title: decision
---

REJECT CRDT (Automerge/Yjs): opaque binary docs DON'T git-diff/PR-review (binary-in-git again), heavy Rust dep, automerge-wasm in QuickJS breaks native+JS parity (arch rule #2). Conflict-freedom already achieved cheaper by immutable-event files. Only justified for real-time multi-cursor co-edit of ONE body, which §17 single-writer-per-body avoids.
