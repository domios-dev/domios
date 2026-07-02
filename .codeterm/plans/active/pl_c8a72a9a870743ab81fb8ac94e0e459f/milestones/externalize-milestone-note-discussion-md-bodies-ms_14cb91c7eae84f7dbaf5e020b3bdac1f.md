---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_14cb91c7eae84f7dbaf5e020b3bdac1f
handle: externalize-milestone-note-discussion-md-bodies-ms_14cb91c7eae84f7dbaf5e020b3bdac1f
title: status_update
---

Accepted externalize-md-bodies lane. Integrated milestone sidecars under milestones/<task-handle>-<milestone-id>.md and note/decision/discussion element sidecars under elements/<kind>-<element-id>.md; JSON strips only externalized prose while short one-line fields stay inline; load hydrates and migrates legacy inline markdown; file manifest and INDEX.md include milestone/element sidecars. NoteCard/DecisionCard now read string content plus object text/body/markdown fields. Gates: cargo fmt --manifest-path src-tauri/Cargo.toml -- --check passed; perl -e 'alarm 120; exec @ARGV' cargo test --manifest-path src-tauri/Cargo.toml --lib body_file -- --nocapture passed (4 tests); npx tsc --noEmit passed; npx tsx src/components/Canvas/cards/cards.test.tsx passed (18 tests); CODETERM_API_PORT=7686 ./build.sh check passed; git diff --check passed. File-size guard: lane files under limit (body_files.rs 630 lines, body_file_tests.rs 292); script still reports unrelated pre-existing oversized files and src-tauri/src/mem/store/tests.rs baseline growth from other work.
